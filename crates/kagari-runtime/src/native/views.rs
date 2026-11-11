//! Call-scoped handles borrow existing frame roots; buffer access stays scoped.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, leases::BorrowedLease, roots::RootedValue},
    native::{
        binding::NativeResult,
        context::{ArgumentView, CallContext},
        scalar::NativeScalar,
        storage::NativePayload,
    },
    value::Value,
};
use kagari_types::ty::Ty;

/// A generic value keeps the exact closed Kagari type of its rooted argument.
/// Use a scalar copy or an explicit owning root when retaining it in Rust.
pub struct ValueHandle<'call> {
    heap: &'call GcHeap,
    arguments: ArgumentView<'call>,
    slot: usize,
    ty: &'call Ty,
}

/// Prevents recursive entry into an explicitly guarded native payload operation.
/// The receiver remains protected by its call argument; no payload borrow spans reentry.
pub struct NativeOperationGuard<'guard> {
    _receiver: &'guard ValueHandle<'guard>,
    _lease: BorrowedLease<'guard>,
}

impl<'call> ValueHandle<'call> {
    /// Returns None when the same receiver already has a guarded operation active.
    /// The library chooses its recursion error. Drop releases the marker on every exit.
    pub fn try_enter_operation(&self) -> NativeResult<Option<NativeOperationGuard<'_>>> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native operation argument"));
        };
        self.heap.try_enter_native_operation(id).map(|lease| {
            lease.map(|lease| NativeOperationGuard {
                _receiver: self,
                _lease: lease,
            })
        })
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

    /// This copy remains protected by the argument only while that argument is
    /// alive. Root it before retaining it beyond the call or mutable reentry.
    pub fn value(&self) -> Value {
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
        self.heap.ensure_no_native_borrow()?;
        self.heap
            .root_value(self.value())
            .ok_or_else(|| RuntimeError::module_validation("native owning value root"))
    }

    pub fn with_payload<S: NativePayload, R>(
        &self,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.value() else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        self.heap.with_native(id, access)
    }
}
