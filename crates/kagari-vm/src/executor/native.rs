//! Native-to-script reentry executes the exact selection prepared by Runtime.
use crate::{error::VmError, executor::Executor};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, reason::IndexError, trap::TrapReason},
    module::ExecutableModule,
    native::{binding::NativeResult, context::ScriptCall},
    value::Value,
};

pub(crate) fn invoke_script(
    runtime: &Runtime,
    _owner: &ExecutableModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    let stack = match target {
        ScriptCall::Closure(closure) => {
            let stack = runtime.enter_closure_execution_stack(closure)?;
            stack.push_closure(runtime, closure.value(), arguments, None)?;
            stack
        }
        ScriptCall::Pinned(function) => {
            let stack = runtime.enter_pinned_execution_stack(function)?;
            stack.push_pinned_call(runtime, function, arguments)?;
            stack
        }
        ScriptCall::Interface(method) => {
            let stack = runtime.enter_interface_execution_stack(method)?;
            stack.push_interface_method(runtime, method.clone(), arguments, None)?;
            stack
        }
    };
    Executor {
        runtime,
        stack: Some(stack),
    }
    .run()
    .map_err(native_error)
}

pub(crate) fn native_error(error: VmError) -> RuntimeError {
    match error {
        VmError::Traced { error, trace } => native_error(*error).with_trace(trace),
        VmError::RuntimeError(error) => error,
        VmError::HostError(error) => RuntimeError::from_host(error),
        VmError::Trap(message) | VmError::TypeMismatch(message) => {
            RuntimeError::from(TrapReason::Provider(message.into()))
        }
        VmError::InvalidIndex(index) => RuntimeError::from(IndexError::InvalidIndex { index }),
        error => RuntimeError::module_validation(format!("native callback execution: {error:?}")),
    }
}
