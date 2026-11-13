use crate::{
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    executor::Executor,
    host::HostCallContext,
    module::ExecutableModule,
    native::{conversion::arguments::ArgumentValues, conversion::context::ConversionContext},
    value::carriers::RootedValue,
};

use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;

/// Synchronously select the current implementation of a checked function.
/// Host callbacks keep returned heap objects alive through this owned root.
pub fn reenter(
    context: &mut HostCallContext<'_>,
    loaded: &ExecutableModule,
    function: FunctionRef,
    args: ArgumentValues<'_>,
) -> Result<RootedValue, RuntimeError> {
    let runtime = context.runtime_mut();
    let invalid = |message| RuntimeError::module_validation(message);
    runtime
        .execution_root()
        .ok_or_else(|| invalid("script reentry requires an active root call"))?;
    runtime.validate_executable_module(loaded)?;
    let route = loaded.bind_call_route(CallableTarget::Script(function))?;
    let selected = runtime.select_call_route(&route)?;
    let loaded = selected.owner();
    let CallableTarget::Script(function) = selected.target() else {
        return Err(invalid("script reentry selected a native import"));
    };
    let mut scope = runtime.begin_execution(loaded, runtime.execution_options())?;
    let runtime = scope.runtime_mut();
    let target = loaded
        .bytecode
        .functions
        .get(function.index())
        .ok_or(RuntimeError::from(
            EngineFault::VerifiedFunctionReferenceIsMissing,
        ))?;
    if args.len() != target.metadata.params.len() {
        return Err(invalid(
            "reentry arguments do not match the linked signature",
        ));
    }
    let resources = &runtime.resources;
    let valid = runtime
        .memory
        .scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                args.check_bounds(slots)?;
                for (index, ty) in target.metadata.params.iter().enumerate() {
                    if index != 0
                        && index.is_multiple_of(1024)
                        && let Err(error) = resources.poll_execution()
                    {
                        return Ok(Err(error));
                    }
                    if !args.scoped(slots, index)?.has_representation(*ty) {
                        return Ok(Ok(false));
                    }
                }
                Ok(Ok(true))
            })
        })
        .map_err(|error| runtime.managed_memory_error(error))??;
    if !valid {
        return Err(invalid(
            "reentry arguments do not match the linked signature",
        ));
    }
    Executor::new(runtime, loaded, function, args)?.run(|runtime, stack, value| {
        stack.with_result(runtime, value, |runtime, value| {
            if !value.has_representation(target.metadata.return_type) {
                return Err(invalid(
                    "reentry result does not match the linked signature",
                ));
            }
            ConversionContext::new(runtime, loaded)?.retain(&value)
        })
    })
}
