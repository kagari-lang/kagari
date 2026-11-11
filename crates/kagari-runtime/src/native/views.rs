//! Call-scoped handles borrow existing frame roots; buffer access stays scoped.
use crate::{
    error::RuntimeError,
    gc::{HeapObjectId, roots::RootedValue},
    native::{
        binding::NativeResult,
        context::{ArgumentView, NativeCall},
        scalar::NativeScalar,
        storage::NativePayload,
    },
    value::Value,
};
use kagari_types::ty::Ty;

/// A generic value keeps the exact closed Kagari type of its rooted argument.
/// Use a scalar copy or an explicit owning root when retaining it in Rust.
pub struct ValueHandle<'call> {
    arguments: ArgumentView<'call>,
    slot: usize,
    ty: &'call Ty,
}

/// Prevents recursive entry into an explicitly guarded native payload operation.
/// The receiver remains protected by its call argument; no payload borrow spans reentry.
struct NativeOperationGuard<'guard, 'call> {
    call: &'guard mut NativeCall<'call>,
    receiver: HeapObjectId,
}

impl Drop for NativeOperationGuard<'_, '_> {
    fn drop(&mut self) {
        self.call.heap().leave_native_operation(self.receiver);
    }
}

impl<'call> ValueHandle<'call> {
    /// Run one guarded operation on this receiver. Recursive entry returns None.
    /// The scope owns cleanup through normal return, errors and unwinding, and
    /// lends the same call services to synchronous script reentry.
    pub fn with_operation<R>(
        &self,
        cx: &mut NativeCall<'_>,
        operation: impl FnOnce(&mut NativeCall<'_>) -> NativeResult<R>,
    ) -> NativeResult<Option<R>> {
        let Value::GcHandle(id) = self.value(cx)? else {
            return Err(RuntimeError::module_validation("native operation argument"));
        };
        if !cx.heap().try_enter_native_operation(id)? {
            return Ok(None);
        }
        let scope = NativeOperationGuard {
            call: cx,
            receiver: id,
        };
        operation(scope.call).map(Some)
    }

    pub(crate) fn from_argument(cx: &NativeCall<'call>, slot: usize) -> NativeResult<Self> {
        if !cx.arguments().contains(cx.heap(), slot) {
            return Err(RuntimeError::module_validation("native argument slot"));
        }
        Ok(Self {
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
    pub fn value(&self, cx: &NativeCall<'_>) -> NativeResult<Value> {
        self.arguments.get(cx.heap(), self.slot).ok_or_else(|| {
            RuntimeError::module_validation("foreign or expired native argument slot")
        })
    }

    pub fn scalar<S: NativeScalar>(&self, cx: &NativeCall<'_>) -> NativeResult<S> {
        if self.ty != &Ty::Builtin(S::abi_type()) {
            return Err(RuntimeError::module_validation(
                "native scalar differs from its argument type",
            ));
        }
        S::decode(self.value(cx)?)
    }

    pub fn root(&self, cx: &NativeCall<'_>) -> NativeResult<RootedValue> {
        cx.heap().ensure_no_native_borrow()?;
        cx.heap()
            .root_value(self.value(cx)?)
            .ok_or_else(|| RuntimeError::module_validation("native owning value root"))
    }

    pub fn with_payload<S: NativePayload, R>(
        &self,
        cx: &NativeCall<'_>,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        let Value::GcHandle(id) = self.value(cx)? else {
            return Err(RuntimeError::module_validation("native storage argument"));
        };
        cx.heap().with_native(id, access)
    }
}
