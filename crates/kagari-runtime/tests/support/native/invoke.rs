//! Scoped native fixture metadata and the ordinary execution frame driver.
use kagari_bytecode::{instruction::NativeImportId, module::CallableTarget};
use kagari_runtime::{
    Runtime,
    frame::driver::ReturnAction,
    module::ExecutableModule,
    native::{binding::NativeResult, context::ScriptInvoker},
    value::Value,
};

/// Enter a linked native callable with frame roots, session checks and ordinary cleanup.
pub fn invoke(
    runtime: &Runtime,
    owner: &ExecutableModule,
    slot: usize,
    args: &[Value],
) -> NativeResult<Value> {
    let stack = runtime.enter_execution_stack(owner)?;
    unsafe {
        stack.push_callable_transport(
            runtime,
            owner.clone(),
            CallableTarget::Native(NativeImportId::new(slot)),
            args,
            None,
        )
    }?;
    stack.start_native_entry(runtime, unsafe {
        // SAFETY: The callback rejects execution or returns only a scalar; it retains no raw edges.
        ScriptInvoker::new(|_, _, _, _| unreachable!("fixture does not call script"))
    })?;
    let ReturnAction::Complete(value) = unsafe { stack.finish_native_return_transport(runtime) }?
    else {
        panic!("root native return");
    };
    Ok(value)
}
