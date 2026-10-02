//! Native-to-script reentry uses the existing generation-pinned execution scope.
use crate::{error::VmError, executor::Executor};
use kagari_runtime::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    module::LoadedModule,
    native::{binding::NativeResult, context::ScriptCall},
    value::Value,
};

pub(super) fn invoke_script(
    runtime: &Runtime,
    owner: &LoadedModule,
    target: ScriptCall<'_>,
    arguments: &[Value],
) -> NativeResult<Value> {
    let stack = match target {
        ScriptCall::Selected(target) => {
            let stack = runtime.enter_execution_stack(owner)?;
            stack.push_selected_call(runtime, owner, target, arguments)?;
            stack
        }
        ScriptCall::Closure(closure) => {
            let stack = runtime.enter_closure_execution_stack(closure)?;
            stack.push_closure(runtime, closure.snapshot(), arguments, None)?;
            stack
        }
    };
    Executor { runtime, stack }.run().map_err(native_error)
}

fn native_error(error: VmError) -> RuntimeError {
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
