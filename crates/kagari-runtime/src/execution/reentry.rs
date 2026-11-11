use crate::{
    error::{RuntimeError, fault::EngineFault},
    executor::Executor,
    gc::roots::RootedValue,
    host::HostCallContext,
    module::ExecutableModule,
    value::Value,
};

use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;

/// Synchronously select the current implementation of a checked function.
/// Host callbacks keep returned heap objects alive through this owned root.
pub fn reenter(
    context: &mut HostCallContext<'_>,
    loaded: &ExecutableModule,
    function: FunctionRef,
    args: &[Value],
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
    if args.len() != target.metadata.params.len()
        || args.iter().zip(&target.metadata.params).any(|(value, ty)| {
            !value.has_representation(*ty) || !runtime.gc().validate_value(value)
        })
    {
        return Err(invalid(
            "reentry arguments do not match the linked signature",
        ));
    }
    let value = Executor::new(runtime, loaded, function, args)?.run()?;
    if !value.has_representation(target.metadata.return_type) {
        return Err(invalid(
            "reentry result does not match the linked signature",
        ));
    }
    runtime
        .gc()
        .root_value(value)
        .ok_or_else(|| invalid("reentry result cannot be retained"))
}
