//! Native-to-script reentry executes the exact selection prepared by Runtime.
use crate::{
    Runtime,
    executor::Executor,
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
}
