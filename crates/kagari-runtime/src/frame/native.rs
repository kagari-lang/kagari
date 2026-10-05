//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState, ReturnDestination},
    native::context::{ArgumentSlots, ArgumentView, CallContext, ScriptInvoker},
    value::Value,
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::CallableTarget,
};
use std::rc::Rc;

impl ExecutionStack {
    fn validate_native_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_top()?;
        if !Rc::ptr_eq(&runtime.resources, &self.session.resources) {
            return Err(self
                .session
                .resources
                .quarantine("native invocation used another runtime"));
        }
        Ok(())
    }

    pub fn start_native_entry(
        &self,
        runtime: &Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        self.validate_native_runtime(runtime)?;
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
                frame.slots.clone(),
                frame.environment(),
            )
        };
        let function = loaded
            .native_binding(import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native callable"))?;
        let function =
            if loaded.bytecode.native_imports[import.index()]
                .generic
                .is_some()
            {
                Rc::new(function.apply(
                    runtime,
                    &loaded,
                    environment.ok_or_else(|| {
                        RuntimeError::module_validation("shared native environment")
                    })?,
                )?)
            } else {
                function
            };
        let mut context = CallContext {
            runtime,
            owner: &loaded,
            function: &function,
            invoke_script,
            arguments: ArgumentView::new(
                &self.heap,
                &roots,
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
            .set(&self.heap, 0, value)
            .ok_or_else(|| RuntimeError::module_validation("native return destination"))?;
        frame.native_entry = NativeEntryState::Complete;
        Ok(())
    }

    pub fn invoke_native(
        &self,
        runtime: &Runtime,
        import: NativeImportId,
        arguments: &[Register],
        destination: Option<Register>,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let (loaded, roots) = {
            let frame = self.current()?;
            (frame.loaded.clone(), frame.slots.clone())
        };
        let function = loaded
            .native_binding(import)
            .ok_or_else(|| RuntimeError::module_validation("unlinked native callable"))?;
        let mut context = CallContext {
            runtime,
            owner: &loaded,
            function: &function,
            invoke_script,
            arguments: ArgumentView::new(&self.heap, &roots, ArgumentSlots::Registers(arguments)),
        };
        let value = function.invoke(&mut context)?;
        if let Some(destination) = destination {
            self.current_mut()?.write_register(destination, value)?;
        }
        Ok(())
    }

    pub fn finish_return(
        &self,
        runtime: &Runtime,
        mut value: Value,
    ) -> Result<Option<Value>, RuntimeError> {
        self.validate_native_runtime(runtime)?;
        let destination = {
            let frame = self.current()?;
            if frame.interface_method().is_none()
                && let Some(environment) = frame.environment()
            {
                let ty = match frame.target() {
                    CallableTarget::Script(_) => frame
                        .function()
                        .and_then(|function| function.metadata.semantic.result.as_ref()),
                    CallableTarget::Native(import) => frame
                        .loaded()
                        .bytecode
                        .native_imports
                        .get(import.index())
                        .map(|import| &import.signature.result),
                }
                .ok_or_else(|| RuntimeError::module_validation("shared return contract"))?;
                if !runtime.matches_type_in(&value, ty, frame.loaded(), Some(&environment)) {
                    return Err(RuntimeError::module_validation(
                        "shared return type mismatch",
                    ));
                }
            }
            if let Some(method) = frame.interface_method() {
                value = runtime.finish_interface_method_result(method, value)?;
            }
            frame.return_to
        };
        self.pop()?;
        if self.is_empty()? {
            return Ok(Some(value));
        }
        match destination {
            ReturnDestination::Register(Some(destination)) => {
                self.current_mut()?.write_register(destination, value)?
            }
            ReturnDestination::Register(None) => {}
        }
        Ok(None)
    }
}
