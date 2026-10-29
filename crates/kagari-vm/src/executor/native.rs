//! Native-to-script reentry executes the exact selection prepared by Runtime.
use crate::{error::VmError, executor::Executor};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        context::{ScriptCall, ScriptInvoker},
    },
    value::Value,
};

// SAFETY: invoke_script admits the retained target and publishes all arguments
// into its execution frame before running. Executor transfers the live terminal
// result immediately, with no intervening collection after releasing the frame.
pub(crate) const INVOKE_SCRIPT: ScriptInvoker = unsafe { ScriptInvoker::new(invoke_script) };

fn invoke_script(
    runtime: &Runtime,
    _owner: &ExecutableModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    let stack = match target {
        ScriptCall::Closure(closure) => {
            let stack = runtime.enter_closure_execution_stack(closure)?;
            // SAFETY: The admitted ScriptCall keeps its selected closure root
            // alive until the new frame publishes the closure and its captures.
            unsafe {
                stack.push_closure_transport(runtime, closure.value_transport(), arguments, None)
            }?;
            stack
        }
        ScriptCall::Pinned(function) => {
            let stack = runtime.enter_pinned_execution_stack(function)?;
            unsafe { stack.push_pinned_call_transport(runtime, function, arguments) }?;
            stack
        }
        ScriptCall::Interface(method) => {
            let stack = runtime.enter_interface_execution_stack(method)?;
            unsafe {
                stack.push_interface_method_transport(runtime, method.clone(), arguments, None)
            }?;
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
        VmError::BuiltinError(error) => error.into_runtime_error(),
        VmError::HostError(error) => {
            let result = RuntimeError::host_call_failure(error.message());
            match error.trace() {
                Some(trace) => result.with_trace(trace.clone()),
                None => result,
            }
        }
        VmError::ReflectionError(error) => error.into_runtime_error(),
        VmError::Trap(message) | VmError::TypeMismatch(message) => {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, message)
        }
        VmError::InvalidIndex(index) => RuntimeError::new(
            RuntimeErrorKind::IndexOutOfBounds,
            format!("invalid index: {index}"),
        ),
        error => RuntimeError::module_validation(format!("native callback execution: {error:?}")),
    }
}
