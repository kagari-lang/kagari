//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{
        FrameStack, NativeEntryState,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::linked_execution::LinkedNativeBody,
    native::context::{ArgumentSlots, ArgumentView, NativeCall, ScriptInvoker},
};

use kagari_bytecode::module::CallableTarget;

impl FrameStack {
    pub fn start_native_entry(
        &self,
        runtime: &mut Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<ExecutionAction, RuntimeError> {
        self.validate_top(runtime)?;
        let (loaded, import, roots, environment) = {
            let mut frame = self.current_mut(runtime)?;
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
        let mut context = NativeCall {
            _thread: Default::default(),
            pending: Default::default(),
            runtime,
            owner: &loaded,
            function,
            invoke_script,
            arguments: ArgumentView::frame(
                roots,
                ArgumentSlots::Contiguous {
                    start: 1,
                    count: function.signature.params.len(),
                },
            ),
        };
        let value = function.invoke(&mut context)?;
        let mut frame = self.current_mut(runtime)?;
        frame
            .slots
            .set(&runtime.gc, 0, value)
            .ok_or_else(|| RuntimeError::module_validation("native return destination"))?;
        frame.native_entry = NativeEntryState::Complete;
        Ok(ExecutionAction::NativeReturn)
    }

    /// Consume a completed native frame at an explicit return transition.
    pub fn finish_native_return(&self, runtime: &Runtime) -> Result<ReturnAction, RuntimeError> {
        self.validate_top(runtime)?;
        let value = {
            let frame = self.current(runtime)?;
            if !matches!(frame.native_entry, NativeEntryState::Complete) {
                return Err(runtime
                    .resources()
                    .quarantine(EngineFault::NativeReturnBeforeCompletion));
            }
            frame.slots.get(&runtime.gc, 0).ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidNativeReturnSlot)
            })?
        };
        self.finish_return(runtime, ReturnValue::general(value))
    }

    pub(super) fn invoke_prepared_native(
        &self,
        runtime: &mut Runtime,
        index: usize,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        // execute_region admitted this runtime/session; no callback or collection
        // intervened. Retain the linked descriptor and roots, releasing all frame
        // and bank borrows before invoking arbitrary Rust or reentrant script.
        let (loaded, roots, links) = {
            let frame = self.current(runtime)?;
            (frame.loaded.clone(), frame.slots, frame.links.clone())
        };
        let operation = links
            .as_ref()
            .and_then(|links| links.native_call(index))
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::MissingPreparedNativeCall)
            })?;
        let LinkedNativeBody::Callback(callback) = &operation.body else {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::SealedNativeBodyReachedCallbackTransition));
        };
        let mut context = NativeCall {
            _thread: Default::default(),
            pending: Default::default(),
            runtime,
            owner: &loaded,
            function: &callback.function,
            invoke_script,
            arguments: ArgumentView::frame(roots, ArgumentSlots::Prepared(&callback.arguments)),
        };
        let value = callback.function.invoke(&mut context)?;
        let mut frame = self.current_mut(runtime)?;
        if let Some(destination) = operation.destination {
            frame
                .slots
                .set_location(&runtime.gc, destination, value)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine(EngineFault::InvalidNativeReturnDestination)
                })?;
        }
        frame.completed = frame.executing;
        Ok(())
    }
}
