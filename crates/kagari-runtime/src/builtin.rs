use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind, context::ErrorLocation, trap::TrapReason},
    module::ExecutableModule,
    value::Value,
};
pub mod standard;

use kagari_contract::standard::RuntimePrimitive;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct BuiltinError {
    #[source]
    error: RuntimeError,
}

impl BuiltinError {
    pub fn new(reason: TrapReason) -> Self {
        Self {
            error: RuntimeError::from(reason),
        }
    }

    pub fn into_runtime_error(self) -> RuntimeError {
        self.error
    }

    pub fn kind(&self) -> RuntimeErrorKind {
        self.error.kind()
    }

    fn with_context(self, intrinsic: RuntimePrimitive) -> Self {
        self.error.at(ErrorLocation::Builtin(intrinsic)).into()
    }
}

impl From<RuntimeError> for BuiltinError {
    fn from(error: RuntimeError) -> Self {
        Self { error }
    }
}

pub fn invoke_standard(
    runtime: &Runtime,
    owner: &ExecutableModule,
    intrinsic: RuntimePrimitive,
    args: &[Value],
) -> Result<Value, BuiltinError> {
    standard::invoke(runtime, owner, intrinsic, args).map_err(|err| err.with_context(intrinsic))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{error::reason::ErrorReason, error_trace::ErrorTrace};
    use kagari_types::{arithmetic::ArithmeticError, scalar::error::ScalarError};
    use std::sync::Arc;

    #[test]
    fn builtin_context_preserves_the_reason_and_existing_trace() {
        let trace = Arc::new(ErrorTrace {
            incomplete: true,
            ..Default::default()
        });
        let reason = ScalarError::from(ArithmeticError::Overflow);
        let error = RuntimeError::from(
            BuiltinError::from(RuntimeError::from(reason).with_trace(trace.clone()))
                .with_context(RuntimePrimitive::ValueCmp),
        );
        assert_eq!(error.reason(), &ErrorReason::Scalar(reason));
        assert_eq!(error.to_string(), "ValueCmp: integer overflow");
        assert!(Arc::ptr_eq(error.trace().unwrap(), &trace));
    }
}
