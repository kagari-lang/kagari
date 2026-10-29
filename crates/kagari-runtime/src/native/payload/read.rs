//! Callback-free receiver resolution inside the payload read admission boundary.
use crate::{
    Runtime,
    error::RuntimeError,
    gc::reference::GcRef,
    native::{
        binding::NativeResult,
        context::CallContext,
        payload::{Opaque, borrowed::BorrowedPayload},
        storage::NativePayload,
    },
    value::Value,
};

/// Closed receiver forms let the read kernel poll before validating the anchor
/// without lending an admission token or invoking a caller-supplied resolver.
/// Resolution cannot execute provider code, collect, or change execution state.
pub(crate) enum ReadReceiver<'view, P> {
    Borrowed(&'view BorrowedPayload<'view, P>),
    Opaque(&'view Opaque<P>),
    Argument(&'view CallContext<'view>, usize),
    Value(GcRef),
}

impl<P: NativePayload> ReadReceiver<'_, P> {
    pub(crate) fn resolve(self, runtime: &Runtime) -> NativeResult<GcRef> {
        let value = match self {
            Self::Borrowed(payload) => payload.value(runtime)?,
            Self::Opaque(payload) => payload.value(runtime.gc())?,
            Self::Argument(call, index) => {
                let Value::GcHandle(id) = call.argument(index)? else {
                    return Err(RuntimeError::module_validation("native storage argument"));
                };
                return Ok(id);
            }
            Self::Value(id) => return Ok(id),
        };
        let Value::GcHandle(id) = value else {
            unreachable!("checked native payload anchor")
        };
        Ok(id)
    }
}
