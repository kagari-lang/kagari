//! Scoped native fixture metadata and the ordinary execution frame driver.
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_runtime::{Runtime, module::LoadedModule, native::binding::NativeResult, value::Value};

/// Enter a linked native callable with frame roots, session checks and ordinary cleanup.
pub fn invoke(
    runtime: &Runtime,
    owner: &LoadedModule,
    slot: usize,
    args: &[Value],
) -> NativeResult<Value> {
    let stack = runtime.enter_execution_stack(owner)?;
    stack.push_callable(
        runtime,
        owner.clone(),
        CallableTarget::Native(NativeImportId::new(slot)),
        args,
        None,
    )?;
    stack.start_native_entry(runtime, |_, _, _, _| {
        unreachable!("fixture does not call script")
    })?;
    Ok(stack
        .finish_native_return(runtime)?
        .expect("root native return"))
}
