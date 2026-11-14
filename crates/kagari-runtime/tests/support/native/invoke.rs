//! Scoped native fixture metadata and the ordinary execution frame driver.
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::driver::ReturnAction,
    module::ExecutableModule,
    native::{binding::NativeResult, conversion::context::ConversionContext},
    value::carriers::RootedValue,
};

/// Enter the actual Native frame driver from explicit Host inputs. Result
/// retention happens before its execution/source activation retires.
pub fn invoke(
    runtime: &mut Runtime,
    owner: &ExecutableModule,
    slot: usize,
    args: &[RootedValue],
) -> NativeResult<RootedValue> {
    let mut conversion = ConversionContext::new(runtime, owner)?;
    let mut views = Vec::new();
    views
        .try_reserve_exact(args.len())
        .map_err(|_| RuntimeError::resource_limit("fixture argument views"))?;
    for value in args {
        views.push(conversion.import(value)?);
    }
    let arguments = conversion.argument_values(&views)?;
    let mut activation = conversion.runtime_mut().enter_execution_stack(owner)?;
    activation.with(|runtime, stack| {
        stack.push_callable(
            runtime,
            owner.clone(),
            CallableTarget::Native(NativeImportId::new(slot)),
            arguments,
            None,
        )?;
        stack.start_native_entry(runtime, |_, _, _, _, _| {
            unreachable!("fixture does not call script")
        })?;
        let ReturnAction::Complete(value) = stack.finish_native_return(runtime)? else {
            panic!("root native return");
        };
        stack.retain_result(runtime, value)
    })
}
