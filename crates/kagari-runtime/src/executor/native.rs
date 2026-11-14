//! Native-to-script reentry executes the exact selection prepared by Runtime.
use crate::{
    Runtime,
    executor::Executor,
    frame::arguments::FrameArguments,
    module::ExecutableModule,
    native::{binding::NativeResult, context::ScriptCall, conversion::arguments::ArgumentValues},
    value::place::{PublishedResult, ResultPlace},
};

pub(crate) fn invoke_script<'result>(
    runtime: &mut Runtime,
    _owner: &ExecutableModule,
    target: ScriptCall<'_, '_>,
    arguments: ArgumentValues<'_>,
    destination: ResultPlace<'result>,
) -> NativeResult<PublishedResult<'result>> {
    let stack = match target {
        ScriptCall::Selected(selected) => {
            let mut stack = runtime.enter_selected_execution_stack(selected)?;
            stack
                .with(|runtime, frames| frames.push_selected_call(runtime, selected, arguments))?;
            stack
        }
        ScriptCall::Closure(closure) => {
            let mut stack = runtime.enter_closure_execution_stack(closure)?;
            stack.with(|runtime, frames| {
                frames
                    .push_selected_closure(
                        runtime,
                        closure,
                        FrameArguments::pack(arguments),
                        None,
                        None,
                    )
                    .map(|_| ())
            })?;
            stack
        }
        ScriptCall::Pinned(function) => {
            let mut stack = runtime.enter_pinned_execution_stack(function)?;
            stack.with(|runtime, frames| frames.push_pinned_call(runtime, function, arguments))?;
            stack
        }
        ScriptCall::Interface(method) => {
            let mut stack = runtime.enter_interface_execution_stack(method)?;
            stack.with(|runtime, frames| {
                frames.push_interface_method(runtime, method, arguments, None)
            })?;
            stack
        }
    };
    Executor::from_activation(stack).run(|runtime, stack, value| {
        stack.with_result(runtime, value, |runtime, value| {
            destination.publish(runtime, &value)
        })
    })
}
