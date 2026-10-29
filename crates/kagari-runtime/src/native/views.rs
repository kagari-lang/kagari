//! Call-scoped handles borrow existing frame roots; buffer access stays scoped.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, leases::BorrowedLease, roots::RootedValue},
    native::{
        binding::NativeResult,
        context::{CallContext, arguments::ArgumentView},
        scalar::NativeScalar,
        storage::{NativePayload, access::StorageAccess},
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::any::Any;

/// A generic value keeps the exact closed Kagari type of its rooted argument.
/// Use a scalar copy or an explicit owning root when retaining it in Rust.
pub struct ValueHandle<'call> {
    heap: &'call GcHeap,
    arguments: ArgumentView<'call>,
    slot: usize,
    ty: &'call Ty,
}

/// Which callback accesses an operation permits without its owning guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOperationKind {
    ExclusiveSlots,
    ReadOnlyCallbacks,
}

pub(crate) type Recovery = dyn Fn(&mut (dyn Any + Send), &mut StorageAccess<'_>);

/// Retains the receiver and seals recovery before releasing its exclusion lease.
/// No context or payload borrow spans callback execution.
pub struct NativeOperationGuard<'guard> {
    pub(crate) heap: &'guard GcHeap,
    pub(crate) id: HeapObjectId,
    pub(crate) runtime: Option<&'guard Runtime>,
    pub(crate) recovery: Option<Box<Recovery>>,
    pub(crate) _root: Option<RootedValue>,
    pub(crate) _lease: BorrowedLease<'guard>,
}

impl Drop for NativeOperationGuard<'_> {
    fn drop(&mut self) {
        if let (Some(runtime), Some(recovery)) = (self.runtime, self.recovery.as_ref()) {
            self.heap
                .recover_native(runtime, self.id, recovery.as_ref());
        } else if self.heap.native_operation_kind(self.id)
            == Some(NativeOperationKind::ExclusiveSlots)
        {
            self.heap.finish_native_operation(self.id);
        }
    }
}

impl<'call> ValueHandle<'call> {
    /// Returns None when the same receiver already has a guarded operation active.
    /// The library chooses its recursion error. Drop releases the marker on every exit.
    pub fn try_enter_operation(&self) -> NativeResult<Option<NativeOperationGuard<'_>>> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native operation argument"));
        };
        self.heap.prepare_native_recovery(id)?;
        let Some(lease) = self
            .heap
            .try_enter_native_operation(id, NativeOperationKind::ExclusiveSlots)?
        else {
            return Ok(None);
        };
        let root = self.root()?;
        Ok(Some(NativeOperationGuard {
            heap: self.heap,
            id,
            runtime: None,
            recovery: None,
            _root: Some(root),
            _lease: lease,
        }))
    }

    pub(crate) fn from_argument(cx: &CallContext<'call>, slot: usize) -> NativeResult<Self> {
        if !cx.arguments().contains(slot) {
            return Err(RuntimeError::module_validation("native argument slot"));
        }
        Ok(Self {
            heap: cx.heap(),
            arguments: cx.arguments(),
            slot,
            ty: cx.argument_type(slot)?,
        })
    }

    pub fn declared_type(&self) -> &'call Ty {
        self.ty
    }

    /// Extract provider transport while the source argument remains protected.
    ///
    /// # Safety
    /// Retain an independent owner before a copy can outlive the argument view or
    /// its source can be displaced. Scoped host capabilities cannot outlive their
    /// original scope. Use root() or scalar() for ordinary safe host reads.
    #[doc(hidden)]
    pub unsafe fn value_transport(&self) -> Value {
        self.value()
    }

    pub(crate) fn value(&self) -> Value {
        self.arguments
            .get(self.slot)
            .expect("checked native argument slot")
    }

    pub fn scalar<S: NativeScalar>(&self) -> NativeResult<S> {
        if self.ty != &Ty::Builtin(S::abi_type()) {
            return Err(RuntimeError::module_validation(
                "native scalar differs from its argument type",
            ));
        }
        S::decode(self.value())
    }

    pub fn root(&self) -> NativeResult<RootedValue> {
        self.heap.ensure_no_storage_borrow()?;
        self.heap
            .root_value(self.value())
            .ok_or_else(|| RuntimeError::module_validation("native owning value root"))
    }

    /// # Safety
    /// Extracted edges must be retained before the argument root can expire; interior
    /// mutation must preserve the provider's accounting and publication contract.
    /// Read-only operation protection forbids interior scalar writes too.
    pub unsafe fn with_payload<S: NativePayload, R>(
        &self,
        operation: Option<&NativeOperationGuard<'_>>,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap.with_native_operation(id, operation, access)
    }
}
