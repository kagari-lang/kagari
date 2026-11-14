//! Call-scoped handles borrow source protection; payload access stays scoped.
use crate::{
    error::{RuntimeError, fault::EngineFault},
    native::{
        binding::NativeResult, context::NativeCall, scalar::NativeScalar, storage::NativePayload,
    },
    value::{carriers::RootedValue, view::ValueView},
};
use kagari_gc::heap::Phase;
use kagari_types::ty::Ty;

/// A generic value keeps the exact closed Kagari type of its protected argument.
/// Use a scalar copy or an explicit owning root when retaining it in Rust.
pub struct ValueHandle<'call> {
    value: ValueView<'call>,
    ty: &'call Ty,
}

/// The source coordinate survives collection/reentry without a payload pointer,
/// owning root or a source/heap loan across arbitrary executing services.
struct NativeOperationGuard<'guard, 'call> {
    call: &'guard mut NativeCall<'call>,
    receiver: ValueView<'guard>,
}

impl Drop for NativeOperationGuard<'_, '_> {
    fn drop(&mut self) {
        match self.call.runtime.memory.stats().phase {
            Phase::Closed => return,
            Phase::Faulted => {
                if !self.call.runtime.resources.is_quarantined() {
                    self.call
                        .runtime
                        .resources
                        .quarantine(EngineFault::NativeOperationCleanupFailed);
                }
                return;
            }
            _ => {}
        }
        if self.call.leave_native_operation(self.receiver).is_err()
            && !self.call.runtime.resources.is_quarantined()
        {
            self.call
                .runtime
                .resources
                .quarantine(EngineFault::NativeOperationCleanupFailed);
        }
    }
}

impl<'call> ValueHandle<'call> {
    /// Run one guarded operation on this receiver. Recursive entry returns None.
    /// Normal return, errors and unwinding clear the flag before the argument's
    /// source owner retires. No Rust payload reference spans script reentry.
    pub fn with_operation<'invoke, R>(
        &self,
        cx: &mut NativeCall<'invoke>,
        operation: impl FnOnce(&mut NativeCall<'invoke>) -> NativeResult<R>,
    ) -> NativeResult<Option<R>> {
        let receiver = self.value(cx)?;
        if !cx.enter_native_operation(receiver)? {
            return Ok(None);
        }
        let scope = NativeOperationGuard {
            call: cx,
            receiver: receiver.reborrow(),
        };
        operation(scope.call).map(Some)
    }

    pub(crate) fn from_argument(cx: &mut NativeCall<'call>, slot: usize) -> NativeResult<Self> {
        let value = cx.argument(slot)?;
        Ok(Self {
            value,
            ty: cx.argument_type(slot)?,
        })
    }

    pub fn declared_type(&self) -> &'call Ty {
        self.ty
    }

    /// The original argument owner protects this view across synchronous reentry.
    /// Source admission rejects foreign or retired owners before returning it.
    pub fn value(&self, cx: &mut NativeCall<'_>) -> NativeResult<ValueView<'call>> {
        cx.validate_value(self.value)
    }

    pub fn scalar<S: NativeScalar>(&self, cx: &mut NativeCall<'_>) -> NativeResult<S> {
        if self.ty != &Ty::Builtin(S::abi_type()) {
            return Err(RuntimeError::module_validation(
                "native scalar differs from its argument type",
            ));
        }
        S::decode(self.value(cx)?)
    }

    /// Explicit ownership rejects runtime/scoped Host temporaries before root
    /// registration; keeping a wrapper must never extend a borrowed Rust place.
    pub fn root(&self, cx: &mut NativeCall<'_>) -> NativeResult<RootedValue> {
        cx.retain_value(self.value)
    }

    pub fn with_payload<S: NativePayload, R>(
        &self,
        cx: &mut NativeCall<'_>,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        cx.read_payload(self.value, access)
    }
}
