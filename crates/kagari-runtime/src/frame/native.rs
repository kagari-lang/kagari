//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState, driver::ExecutionAction, transfer::ReturnValue},
    native::context::{ArgumentSlots, ArgumentView, CallContext, ScriptInvoker},
    value::Value,
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::CallableTarget,
};

impl ExecutionStack<'_> {
    pub fn start_native_entry(
        &self,
        runtime: &Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<ExecutionAction, RuntimeError> {
        self.validate_runtime(runtime)?;
        let (loaded, import, roots, environment) = {
            let mut frame = self.current_mut()?;
            let CallableTarget::Native(import) = frame.target else {
                return Err(RuntimeError::module_validation(
                    "expected native callable frame",
                ));
            };
            if !matches!(frame.native_entry, NativeEntryState::Pending) {
                return Err(RuntimeError::module_validation(
                    "native callable already started",
                ));
            }
            frame.native_entry = NativeEntryState::Running;
            (
                frame.loaded.clone(),
                import,
                frame.slots,
                frame.environment(),
            )
        };
        let function = runtime
            .modules
            .native_binding(&loaded, import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native callable"))?;
        let application =
            if loaded.bytecode.native_imports[import.index()]
                .generic
                .is_some()
            {
                Some(runtime.prepare_native_application(
                    &loaded,
                    import,
                    environment.ok_or_else(|| {
                        RuntimeError::module_validation("shared native environment")
                    })?,
                )?)
            } else {
                None
            };
        if let Some(application) = &application {
            roots.publish_native_application(&runtime.gc, application.clone())?;
        }
        let function = application
            .as_ref()
            .map_or(function.as_ref(), |application| &application.function);
        let mut context = CallContext {
            runtime,
            owner: &loaded,
            function,
            invoke_script,
            arguments: ArgumentView::frame(
                &runtime.gc,
                roots,
                ArgumentSlots::Contiguous {
                    start: 1,
                    count: function.signature.params.len(),
                },
            ),
        };
        let value = function.invoke(&mut context)?;
        let mut frame = self.current_mut()?;
        frame
            .slots
            .set(&runtime.gc, 0, value)
            .ok_or_else(|| RuntimeError::module_validation("native return destination"))?;
        frame.native_entry = NativeEntryState::Complete;
        Ok(ExecutionAction::NativeReturn)
    }

    /// Consume a completed native frame at an explicit return transition.
    pub fn finish_native_return(&self, runtime: &Runtime) -> Result<Option<Value>, RuntimeError> {
        self.validate_runtime(runtime)?;
        let value = {
            let frame = self.current()?;
            if !matches!(frame.native_entry, NativeEntryState::Complete) {
                return Err(runtime
                    .resources()
                    .quarantine("native return before completion"));
            }
            frame
                .slots
                .get(&runtime.gc, 0)
                .ok_or_else(|| runtime.resources().quarantine("invalid native return slot"))?
        };
        self.finish_return(runtime, ReturnValue::general(value))
    }

    pub fn invoke_native(
        &self,
        runtime: &Runtime,
        import: NativeImportId,
        arguments: &[Register],
        destination: Option<Register>,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let (loaded, roots) = {
            let frame = self.current()?;
            (frame.loaded.clone(), frame.slots)
        };
        let function = runtime
            .modules
            .native_binding(&loaded, import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native callable"))?;
        let mut context = CallContext {
            runtime,
            owner: &loaded,
            function: &function,
            invoke_script,
            arguments: ArgumentView::frame(&runtime.gc, roots, ArgumentSlots::Registers(arguments)),
        };
        let value = function.invoke(&mut context)?;
        if let Some(destination) = destination {
            self.current_mut()?
                .write_register(runtime, destination, value)?;
        }
        Ok(())
    }
}
