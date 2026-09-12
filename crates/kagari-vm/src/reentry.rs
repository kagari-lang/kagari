use kagari_ir::bytecode::FunctionRef;
use kagari_runtime::{
    LoadedModule, ModuleInitializationState, RuntimeError, gc::RootedValue, host::HostCallContext,
    value::Value,
};

use crate::{VmError, executor::Executor};

/// Synchronously call an initialized function in the root call's pinned program.
/// Host callbacks keep returned heap objects alive through this owned root.
pub fn reenter(
    context: &HostCallContext<'_>,
    loaded: &LoadedModule,
    function: FunctionRef,
    args: &[Value],
) -> Result<RootedValue, VmError> {
    let runtime = context.runtime();
    let invalid = |message| VmError::RuntimeError(RuntimeError::module_validation(message));
    runtime
        .execution_root()
        .ok_or_else(|| invalid("script reentry requires an active root call"))?;
    let _scope = runtime
        .begin_execution(loaded, runtime.execution_options())
        .map_err(VmError::RuntimeError)?;
    if !runtime
        .module_instance_snapshot(loaded)
        .is_some_and(|instance| instance.state == ModuleInitializationState::Initialized)
    {
        return Err(invalid("script reentry requires an initialized module"));
    }
    let target = loaded
        .bytecode
        .functions
        .get(function.index())
        .ok_or(VmError::InvalidFunctionRef(function))?;
    if args.len() != target.metadata.params.len()
        || args.iter().zip(&target.metadata.params).any(|(value, ty)| {
            !value.has_representation(*ty) || !runtime.gc().validate_value(value)
        })
    {
        return Err(invalid(
            "reentry arguments do not match the linked signature",
        ));
    }
    let mut executor = Executor::new(runtime, loaded, function, args, None)?;
    let value = executor.run()?;
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
