//! Scoped provider access borrows existing roots and checked facts.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    gc::{HeapObjectId, roots::RootedValue},
    native::{
        binding::NativeResult,
        context::{CallContext, arguments::ArgumentView},
        conversion::{KagariType, arguments::KagariArguments},
        function_handle::Function,
        payload::{Opaque, operation_guard, read::ReadReceiver},
        rust_type::check_opaque_mapping,
        storage::{
            NativePayload,
            access::{EditPlan, StorageAccess},
        },
        typed::NativeContext,
        views::{NativeOperationGuard, NativeOperationKind},
    },
    value::Value,
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::ty::Ty;
use std::{marker::PhantomData, ptr};

/// A provider receiver protected by a borrowed argument or owning handle root.
/// This view allocates no root or executable descriptor and cannot outlive its
/// source. Use Opaque for ownership; both use the same payload loan protocol.
pub struct BorrowedPayload<'call, P> {
    runtime: &'call Runtime,
    source: Source<'call>,
    id: HeapObjectId,
    mapping: PhantomData<fn() -> P>,
}

enum Source<'call> {
    Argument(ArgumentView<'call>, usize),
    Owned(&'call RootedValue),
}

impl<P: NativePayload> Opaque<P> {
    /// Borrow the existing owning root for shared provider algorithms without
    /// preparing another descriptor or registering another root.
    pub fn borrow<'view>(
        &'view self,
        cx: &NativeContext<'view>,
    ) -> NativeResult<BorrowedPayload<'view, P>> {
        let Value::GcHandle(id) = self.value(cx.runtime().gc())? else {
            return Err(RuntimeError::module_validation("native storage receiver"));
        };
        Ok(BorrowedPayload {
            runtime: cx.runtime(),
            source: Source::Owned(&self.root),
            id,
            mapping: PhantomData,
        })
    }
}

impl<'call> CallContext<'call> {
    /// Admit a provider payload against this call's checked argument and concrete
    /// Rust storage representation. The resulting view cannot outlive the call.
    pub fn payload<P: NativePayload>(&self, index: usize) -> NativeResult<BorrowedPayload<'_, P>> {
        let Value::GcHandle(id) = self.argument(index)? else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        let expected = self
            .function
            .type_signature()?
            .params
            .get(index)
            .ok_or_else(|| RuntimeError::module_validation("native argument type scope"))?;
        self.heap()
            .check_native_payload_argument::<P>(id, expected, self.owner)?;
        Ok(BorrowedPayload {
            runtime: self.runtime,
            source: Source::Argument(self.arguments(), index),
            id,
            mapping: PhantomData,
        })
    }
}

impl<'view, P: NativePayload> BorrowedPayload<'view, P> {
    /// Admit the mapping and existing root under the context's exact retained owner.
    /// No conversion or callback occurs, so one ordered admission covers the view.
    pub(in crate::native) fn from_root(
        cx: &NativeContext<'view>,
        root: Option<&'view RootedValue>,
        expected: &TypeArgument,
    ) -> NativeResult<Self> {
        let runtime = cx.runtime();
        runtime.gc().ensure_no_storage_borrow()?;
        admit_payload_mapping::<P>(cx, expected)?;
        let root = root.ok_or_else(|| RuntimeError::module_validation("native storage root"))?;
        let Some(Value::GcHandle(id)) = root.value(runtime.gc()) else {
            return Err(RuntimeError::module_validation("native storage root"));
        };
        runtime
            .gc()
            .check_native_payload_argument::<P>(id, expected, cx.owner())?;
        Ok(Self {
            runtime,
            source: Source::Owned(root),
            id,
            mapping: PhantomData,
        })
    }
}

/// Admit executable services independently of the payload's lexical type owner.
/// No user conversion, collector service or store borrow escapes this operation.
pub(in crate::native) fn admit_payload_mapping<P: NativePayload>(
    cx: &NativeContext<'_>,
    expected: &TypeArgument,
) -> NativeResult<()> {
    let runtime = cx.runtime();
    runtime.resources().ensure_execution_allowed()?;
    runtime.validate_executable_module(cx.owner())?;
    // Availability alone permits diagnostic reads after Faulted; publication is
    // required even when this context already retains the exact executable owner.
    runtime.modules().publish_program(cx.owner())?;
    expected.validate(runtime)?;
    let Ty::NativeObject(nominal) = expected.ty() else {
        return Err(RuntimeError::module_validation(
            "native object requires an opaque storage type",
        ));
    };
    check_opaque_mapping::<P>(runtime, nominal.declaration)
}

impl<P: NativePayload> BorrowedPayload<'_, P> {
    /// Inspect immutable builtin evidence without preparing an owning callable.
    /// Generated builtin adapters carry the same evidence as native entries.
    /// Authored callback relationships return None and require ordinary selection.
    pub fn stored_primitive(
        &self,
        cx: &NativeContext<'_>,
        slot: usize,
    ) -> NativeResult<Option<RuntimePrimitive>> {
        self.value(cx.runtime())?;
        Ok(self
            .runtime
            .gc()
            .native_selections(self.id)?
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("stored callable slot"))?
            .primitive())
    }

    pub(super) fn value(&self, runtime: &Runtime) -> NativeResult<Value> {
        let value = match &self.source {
            Source::Argument(arguments, index) => arguments.get(*index),
            Source::Owned(root) => root.value(runtime.gc()),
        };
        if !ptr::eq(runtime, self.runtime) || value != Some(Value::GcHandle(self.id)) {
            return Err(RuntimeError::module_validation(
                "foreign or expired provider argument",
            ));
        }
        Ok(Value::GcHandle(self.id))
    }

    /// Check receiver identity, representation and write exclusion before a
    /// provider invokes callbacks. This grants no loan or mutation authority;
    /// the eventual edit still performs its own resource and revision preflight.
    pub fn check_writable(&self, cx: &NativeContext<'_>) -> NativeResult<()> {
        cx.poll()?;
        self.value(cx.runtime())?;
        let heap = self.runtime.gc();
        heap.ensure_no_storage_borrow()?;
        heap.check_native_payload::<P>(self.id, false)?;
        heap.check_native_operation(self.id, None, true)
    }

    /// Borrow storage through the same checked loan as an owning Opaque handle.
    ///
    /// # Safety
    /// Retain extracted edges before removal or escape from the call. Interior
    /// writes cannot change edges, structure, accounting, or operation protection.
    pub unsafe fn read<R>(
        &self,
        cx: &NativeContext<'_>,
        operation: Option<&NativeOperationGuard<'_>>,
        read: impl for<'a> FnOnce(&'a P, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.runtime()
            .gc()
            .read_native(cx.runtime(), ReadReceiver::Borrowed(self), operation, read)
    }

    /// Edit storage with ordinary preflight, publication, and unwind reconciliation.
    ///
    /// # Safety
    /// Publish imported edges, retain extracted edges before removal, keep all work
    /// state traced, and respect the plan's maximum charge and structural contract.
    pub unsafe fn edit<R>(
        &self,
        cx: &mut NativeContext<'_>,
        operation: Option<&NativeOperationGuard<'_>>,
        plan: EditPlan,
        edit: impl for<'a> FnOnce(&'a mut P, &mut StorageAccess<'a>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.poll()?;
        self.value(cx.runtime())?;
        self.runtime
            .gc()
            .edit_native(self.runtime, self.id, operation, plan, edit)
    }

    /// Prepare a callback-free edit under the same loan that commits it. The
    /// read-only preparation returns the exact occupancy plan and checked facts,
    /// or None when the provider needs a different execution path. Resource and
    /// revision preflight runs after preparation and before the mutable view.
    ///
    /// # Safety
    /// Preparation cannot mutate payload state or let references escape. The edit
    /// must publish imports and respect its plan exactly as for `edit`. Neither
    /// callback may collect or reenter. Extracted facts stay protected by this loan.
    pub unsafe fn edit_prepared<S, R>(
        &self,
        cx: &mut NativeContext<'_>,
        prepare: impl for<'a> FnOnce(
            &'a P,
            &mut StorageAccess<'a>,
        ) -> NativeResult<Option<(EditPlan, S)>>,
        edit: impl for<'a> FnOnce(&'a mut P, &mut StorageAccess<'a>, S) -> NativeResult<R>,
    ) -> NativeResult<Option<R>> {
        cx.poll()?;
        self.value(cx.runtime())?;
        self.runtime
            .gc()
            .edit_native_prepared(self.runtime, self.id, prepare, edit)
    }

    /// Protect this rooted argument across callbacks without creating another root.
    ///
    /// # Safety
    /// Recovery must not allocate, panic, grow charge, execute script, or retain new
    /// edges. Work state stays traced and recovery leaves valid storage/accounting.
    pub unsafe fn try_enter_operation<'view>(
        &'view self,
        cx: &NativeContext<'_>,
        kind: NativeOperationKind,
        recovery: Option<fn(&mut P, &mut StorageAccess<'_>)>,
    ) -> NativeResult<Option<NativeOperationGuard<'view>>> {
        operation_guard(
            self.runtime,
            self.value(cx.runtime())?,
            None,
            kind,
            recovery,
        )
    }

    /// Resolve a stored callable relationship. Ordinary calls follow its live route;
    /// preparing a call pins the selected implementation for one active operation.
    pub fn stored_function<A: KagariArguments, R: KagariType>(
        &self,
        cx: &NativeContext<'_>,
        slot: usize,
    ) -> NativeResult<Function<A, R>> {
        self.value(cx.runtime())?;
        self.runtime
            .gc()
            .native_selections(self.id)?
            .get(slot)
            .ok_or_else(|| RuntimeError::module_validation("stored callable slot"))?
            .function(self.runtime)
    }
}
