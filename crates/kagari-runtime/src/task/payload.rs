//! Task results are traced edges, not independent roots that retain heap cycles.
use crate::native::storage::scan::{NativeScanKind, ScanBudget, ScanStatus};
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::roots::RootedValue,
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        storage::{NativePayload, NativeStorage},
        types::TypeRef,
    },
    resource::HeapCharge,
    task::{ScopeId, TaskFailure, TaskId, control::ScopeControl},
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{
    declaration::{TypeDefKind, native::NativeStorageLayout},
    ty::Ty,
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct TaskPayload {
    pub id: TaskId,
    pub outcome: Option<Result<Value, TaskFailure>>,
}

impl TaskPayload {
    pub(crate) fn outcome_bytes(outcome: Option<&Result<Value, TaskFailure>>) -> usize {
        match outcome {
            Some(Err(failure)) => failure.error.message_capacity(),
            _ => 0,
        }
    }
}

unsafe impl NativePayload for TaskPayload {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::Bounded { max_work: 8 };

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        visit: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        budget.charge(1);
        if let Some(Ok(value)) = &self.outcome {
            visit(value);
        }

        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, Self::outcome_bytes(self.outcome.as_ref()))
    }
}

#[derive(Debug)]
pub(crate) struct ScopePayload {
    pub id: ScopeId,
    pub control: Arc<ScopeControl>,
}

unsafe impl NativePayload for ScopePayload {
    type ScanState = ();
    const SCAN_KIND: NativeScanKind = NativeScanKind::EdgeFree;

    fn begin_scan(&self) {}

    fn scan<'a>(
        &'a self,
        _: &mut Self::ScanState,
        budget: &mut ScanBudget,
        _: &mut dyn FnMut(&'a Value),
    ) -> ScanStatus {
        budget.charge(1);
        ScanStatus::Complete
    }

    fn charge(&self) -> HeapCharge {
        HeapCharge::new(1, 0)
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

    pub(crate) fn allocate_task_payload<S: NativePayload>(
        &self,
        owner: &ExecutableModule,
        ty: TypeArgument,
        payload: S,
    ) -> NativeResult<RootedValue> {
        let Ty::NativeObject(nominal) = ty.ty() else {
            return Err(RuntimeError::module_validation("task storage type"));
        };
        let storage = self
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or_else(|| RuntimeError::module_validation("task storage binding"))?;
        let mut object = storage.prepare_payload(self.gc(), ty.ty(), payload, owner)?;
        object.scope = Some(ty);
        let value = Value::GcHandle(self.gc().alloc_native(object)?);
        self.root_value(value)
            .ok_or_else(|| RuntimeError::module_validation("task storage root"))
    }
}
