use kagari_bytecode::module::CallableTarget;
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    error::RuntimeError, gc::roots::RootedValue, host::HostCallContext, module::ExecutableModule,
    value::Value,
};

use crate::{error::VmError, executor::Executor};

/// Synchronously select the current implementation of a checked function.
/// Host callbacks keep returned heap objects alive through this owned root.
/// Admit protected transport at the backend execution boundary.
///
/// # Safety
/// Keep all managed inputs live until the destination frame or result owner is
/// published. Inputs that can be replaced through aliases require independent
/// protection across callbacks. Root raw results before collection, reentry or
/// source retirement; host capabilities must remain in their original host scope.
/// Existing owner/type/generation checks cannot revive expired direct references.
#[doc(hidden)]
pub unsafe fn reenter_transport(
    context: &HostCallContext<'_>,
    loaded: &ExecutableModule,
    function: FunctionRef,
    args: &[Value],
) -> Result<RootedValue, VmError> {
    reenter(context, loaded, function, args)
}

pub(crate) fn reenter(
    context: &HostCallContext<'_>,
    loaded: &ExecutableModule,
    function: FunctionRef,
    args: &[Value],
) -> Result<RootedValue, VmError> {
    let runtime = context.runtime();
    let invalid = |message| VmError::RuntimeError(RuntimeError::module_validation(message));
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
    let _scope = runtime
        .begin_execution(loaded, runtime.execution_options())
        .map_err(VmError::RuntimeError)?;
    let target = loaded
        .bytecode
        .functions
        .get(function.index())
        .ok_or(VmError::InvalidFunctionRef(function))?;
    if args.len() != target.metadata.params.len()
        || args.iter().zip(&target.metadata.params).any(|(value, ty)| {
            !value.has_representation(*ty)
                || !unsafe { runtime.gc().validate_value_transport(value) }
        })
    {
        return Err(invalid(
            "reentry arguments do not match the linked signature",
        ));
    }
    let mut executor = Executor::new(runtime, loaded, function, args)?;
    let value = executor.run()?;
    if !value.has_representation(target.metadata.return_type) {
        return Err(invalid(
            "reentry result does not match the linked signature",
        ));
    }
    unsafe {
        // SAFETY: Executor transfers its terminal result here without an intervening callback or collection.
        runtime.gc().root_value_transport(value)
    }
    .ok_or_else(|| invalid("reentry result cannot be retained"))
}
