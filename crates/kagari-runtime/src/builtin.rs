use crate::{
    Runtime,
    error::{
        RuntimeError, RuntimeErrorKind, context::ErrorLocation, reason::ErrorReason,
        trap::TrapReason,
    },
    module::ExecutableModule,
    native::conversion::{arguments::ArgumentValues, context::ConversionContext},
    value::place::{PublishedResult, ResultPlace},
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

pub fn invoke_standard<'result>(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    intrinsic: RuntimePrimitive,
    args: ArgumentValues<'_>,
    destination: ResultPlace<'result>,
) -> Result<PublishedResult<'result>, BuiltinError> {
    runtime
        .resources()
        .poll_execution()
        .map_err(|error| attach_error(runtime, error, intrinsic))?;
    let mut conversion = ConversionContext::new(runtime, owner)
        .map_err(|error| BuiltinError::from(error).with_context(intrinsic))?;
    invoke_in(&mut conversion, intrinsic, args.reborrow(), destination)
}

/// Caller-selected execution keeps its real lexical/body origin through builtin
/// construction. Host entry above supplies its own explicit conversion owner.
pub(crate) fn invoke_in<'call, 'result>(
    conversion: &mut ConversionContext<'call>,
    intrinsic: RuntimePrimitive,
    args: ArgumentValues<'call>,
    destination: ResultPlace<'result>,
) -> Result<PublishedResult<'result>, BuiltinError> {
    let result = (|| {
        conversion.poll()?;
        let result = standard::invoke(conversion, intrinsic, args);
        // Post-call termination precedes a business failure; completed effects remain.
        conversion.poll()?;
        let value = result?;
        destination.publish(conversion.runtime_mut(), &value)
    })();
    result.map_err(|error| attach_error(conversion.runtime(), error, intrinsic))
}

fn attach_error(
    runtime: &Runtime,
    error: RuntimeError,
    intrinsic: RuntimePrimitive,
) -> BuiltinError {
    let error = if let ErrorReason::EngineFault(reason) = error.reason() {
        runtime.resources().quarantine(*reason)
    } else {
        error.admit_execution_error(runtime.resources())
    };
    BuiltinError::from(error).with_context(intrinsic)
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
