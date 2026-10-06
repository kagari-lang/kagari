//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState, ReturnDestination, transfer::ReturnValue},
    native::context::{ArgumentSlots, ArgumentView, CallContext, ScriptInvoker},
    value::Value,
};
use kagari_bytecode::{
    instruction::{NativeImportId, Register},
    module::CallableTarget,
};
use std::sync::Arc;

impl ExecutionStack<'_> {
    pub fn start_native_entry(
        &self,
        runtime: &Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
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
        let function =
            if loaded.bytecode.native_imports[import.index()]
                .generic
                .is_some()
            {
                Arc::new(function.apply(
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

    pub fn finish_return(
        &self,
        runtime: &Runtime,
        packet: ReturnValue,
    ) -> Result<Option<Value>, RuntimeError> {
        self.validate_runtime(runtime)?;
        if let Some((representation, bits)) = packet.payload() {
            let destination = {
                let frame = self.current()?;
                (frame.environment().is_none() && frame.interface_method().is_none())
                    .then_some(frame.return_to)
            };
            if let Some(destination) = destination {
                self.pop()?;
                if self.is_empty()? {
                    return packet.materialize().map(Some).ok_or_else(|| {
                        RuntimeError::module_validation("scalar return representation")
                    });
                }
                if let ReturnDestination::Register(Some(register)) = destination {
                    runtime.gc.ensure_execution_allowed()?;
                    let frame = self.current()?;
                    let mut values = runtime
                        .resources()
                        .frame_values
                        .try_borrow_mut()
                        .map_err(|_| runtime.resources().quarantine("return window borrowed"))?;
                    values
                        .set_scalar(frame.slots, register.index(), representation, bits)
                        .ok_or_else(|| {
                            RuntimeError::module_validation("scalar return destination")
                        })?;
                }
                return Ok(None);
            }
        }
        let mut value = packet
            .materialize()
            .ok_or_else(|| RuntimeError::module_validation("return representation"))?;
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
                if !runtime.matches_type_in(&value, ty, frame.loaded(), Some(&environment.types)) {
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
                self.current_mut()?
                    .write_register(runtime, destination, value)?
            }
            ReturnDestination::Register(None) => {}
        }
        Ok(None)
    }
}
