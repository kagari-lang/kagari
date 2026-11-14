//! Sealed Task caches trace results; script Scope capabilities retain status only.
#[cfg(test)]
mod tests;
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::context::{ConversionContext, native::NativeContract},
        storage::NativeStorage,
        types::TypeRef,
    },
    task::{TaskFailure, TaskId, control::ScopeStatus},
    value::{
        carriers::{RootedValue, ScopedValue, StoredValue},
        properties::ValueProperties,
    },
};
use kagari_common::identity::table::DefinitionId;
use kagari_gc::{
    access::Scope,
    error::GcError,
    object::{Gc, GcObject, ObjectCharge},
    scan::{EdgeVisitor, ScanBudget, ScanKind, ScanStatus},
};
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::{fmt::Debug, sync::Arc};

#[derive(Debug)]
enum Outcome {
    Pending,
    Success(StoredValue),
    Failure { failure: TaskFailure, bytes: usize },
}

/// A sealed description sampled outside provider charge/mutation callbacks.
pub(crate) struct PreparedTaskFailure {
    failure: TaskFailure,
    bytes: usize,
}

impl PreparedTaskFailure {
    pub(crate) fn new(failure: TaskFailure) -> Self {
        let bytes = failure.error.retained_bytes();
        Self { failure, bytes }
    }
}

#[derive(Debug)]
pub(crate) struct TaskPayload {
    id: TaskId,
    outcome: Outcome,
}

/// Detached control snapshot. Managed values are copied only through get_result
/// while their retained owner is admitted; no unrooted result leaves this model.
pub(crate) enum TaskCompletion {
    Pending,
    Success,
    Failure(TaskFailure),
}

impl TaskPayload {
    pub(crate) fn alloc<'gc>(cx: &mut Scope<'gc>, id: TaskId) -> Result<Gc<'gc, Self>, GcError> {
        // SAFETY: the pending fixed result coordinate contains no initialized edge.
        unsafe {
            cx.alloc_with(|_| {
                Ok(Self {
                    id,
                    outcome: Outcome::Pending,
                })
            })
        }
    }

    pub(crate) fn id(&self) -> TaskId {
        self.id
    }

    pub(crate) fn is_ready(&self) -> bool {
        !matches!(self.outcome, Outcome::Pending)
    }

    pub(crate) fn completion(&self) -> TaskCompletion {
        match &self.outcome {
            Outcome::Pending => TaskCompletion::Pending,
            Outcome::Success(_) => TaskCompletion::Success,
            Outcome::Failure { failure, .. } => TaskCompletion::Failure(failure.clone()),
        }
    }

    pub(crate) fn get_result<'gc>(
        cx: &Scope<'gc>,
        owner: Gc<'gc, Self>,
    ) -> Result<ScopedValue<'gc>, GcError> {
        StoredValue::get_field(cx, owner, |owner| match &owner.outcome {
            Outcome::Success(value) => Some(value),
            _ => None,
        })
    }

    fn check_pending(&self, id: TaskId) -> Result<(), GcError> {
        if self.id != id || self.is_ready() {
            return Err(GcError::Representation);
        }
        Ok(())
    }

    pub(crate) fn succeed<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        id: TaskId,
        value: ScopedValue<'gc>,
    ) -> Result<(), GcError> {
        cx.read(owner, |owner| owner.check_pending(id))?;
        if !ValueProperties::of(cx, value)?.storable {
            return Err(GcError::Representation);
        }
        // SAFETY: one admitted fixed coordinate; publish before the terminal
        // commit. The target is protected even if this cache was already scanned.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                let value = StoredValue::publish_field(mutation, value)?;
                owner.outcome = Outcome::Success(value);
                mutation.continue_scan();
                Ok(())
            })
        }
    }

    /// bytes is computed before entering provider mutation. Charge itself remains
    /// constant-time, including errors with deep owned diagnostic context.
    pub(crate) fn fail<'gc>(
        cx: &mut Scope<'gc>,
        owner: Gc<'gc, Self>,
        id: TaskId,
        prepared: PreparedTaskFailure,
    ) -> Result<(), GcError> {
        cx.read(owner, |owner| owner.check_pending(id))?;
        let PreparedTaskFailure { failure, bytes } = prepared;
        // SAFETY: no incoming GC edge; admit complete owned bytes before the
        // no-fail terminal commit. Error-only cleanup executes no script/provider.
        unsafe {
            cx.with_object_mut(owner, |owner, mutation| {
                mutation.prepare_charge(ObjectCharge { units: 1, bytes })?;
                owner.outcome = Outcome::Failure { failure, bytes };
                mutation.continue_scan();
                Ok(())
            })
        }
    }
}

// SAFETY: one fixed result coordinate is initialized at most once. Incoming
// references are published, holes are charged and pending visits retry in place.
// Diagnostics own no GC references; charge is a stored scalar, Drop reads no peers.
unsafe impl GcObject for TaskPayload {
    type ScanState = bool;
    const SCAN_KIND: ScanKind = ScanKind::Resumable;

    fn begin_scan(&self) -> bool {
        false
    }

    fn scan(
        &self,
        done: &mut bool,
        budget: &mut ScanBudget,
        visitor: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        if !*done {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            if let Outcome::Success(value) = &self.outcome {
                // SAFETY: retained admitted cache owns this initialized cell.
                if unsafe { value.scan(budget, visitor)? } == ScanStatus::Pending {
                    return Ok(ScanStatus::Pending);
                }
            }
            *done = true;
        }
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge {
            units: 1,
            bytes: match &self.outcome {
                Outcome::Failure { bytes, .. } => *bytes,
                _ => 0,
            },
        }
    }
}

#[derive(Debug)]
pub(crate) struct ScopePayload {
    pub status: Arc<ScopeStatus>,
}

impl ScopePayload {
    pub(crate) fn alloc<'gc>(
        cx: &mut Scope<'gc>,
        status: Arc<ScopeStatus>,
    ) -> Result<Gc<'gc, Self>, GcError> {
        cx.alloc_leaf(Self { status })
    }
}

// SAFETY: the shared external status owns only immutable identity/atomics, no
// dispatcher, subscription, Task root or GC edge. Drop can run no Host callback.
unsafe impl GcObject for ScopePayload {
    type ScanState = ();
    const SCAN_KIND: ScanKind = ScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan(
        &self,
        _: &mut (),
        _: &mut ScanBudget,
        _: &mut EdgeVisitor<'_>,
    ) -> Result<ScanStatus, GcError> {
        Ok(ScanStatus::Complete)
    }

    fn charge(&self) -> ObjectCharge {
        ObjectCharge { units: 1, bytes: 0 }
    }
}

impl NativeStorage {
    pub fn task() -> Self {
        Self::provided_with_layout::<TaskPayload>(NativeStorageLayout::Task)
    }

    pub fn task_scope() -> Self {
        Self::provided_with_layout::<ScopePayload>(NativeStorageLayout::TaskScope)
    }
}

impl DeclarationCatalog {
    fn role_type(&self, layout: NativeStorageLayout) -> NativeResult<TypeRef> {
        let mut found = self
            .types
            .iter()
            .filter(|(_, ty)| ty.kind == TypeDefKind::NativeStorage(layout));
        let (id, _) = found
            .next()
            .ok_or_else(|| RuntimeError::metadata_conflict("missing task storage role"))?;
        if found.next().is_some() {
            return Err(RuntimeError::metadata_conflict(
                "ambiguous task storage role",
            ));
        }
        self.type_reference(&id)
    }

    pub fn task_type(&self) -> NativeResult<TypeRef> {
        self.role_type(NativeStorageLayout::Task)
    }

    pub fn task_scope_type(&self) -> NativeResult<TypeRef> {
        self.role_type(NativeStorageLayout::TaskScope)
    }
}

impl Runtime {
    pub(crate) fn task_role(&self, layout: NativeStorageLayout) -> NativeResult<DefinitionId> {
        let mut ids = self.native_entries.catalog.types.ids().filter(|id| {
            self.native_entries
                .catalog
                .types
                .get_id(*id)
                .is_some_and(|ty| ty.kind == TypeDefKind::NativeStorage(layout))
        });
        let id = ids
            .next()
            .ok_or_else(|| RuntimeError::metadata_conflict("missing task storage role"))?;
        if ids.next().is_some() {
            return Err(RuntimeError::metadata_conflict(
                "ambiguous task storage role",
            ));
        }
        Ok(id)
    }

    pub(crate) fn allocate_task_payload<S: GcObject + Debug>(
        &mut self,
        owner: &ExecutableModule,
        ty: TypeArgument,
        initialize: impl for<'gc> FnOnce(&mut Scope<'gc>) -> Result<Gc<'gc, S>, GcError>,
    ) -> NativeResult<RootedValue> {
        let Ty::NativeObject(nominal) = ty.ty() else {
            return Err(RuntimeError::module_validation("task storage type"));
        };
        let storage = self
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .cloned()
            .ok_or_else(|| RuntimeError::module_validation("task storage binding"))?;
        let mut conversion = ConversionContext::new(self, owner)?;
        conversion.create_native_with(
            NativeContract {
                storage: &storage,
                owner,
                applied: &ty,
            },
            |init| initialize(init.scope()),
        )
    }
}
