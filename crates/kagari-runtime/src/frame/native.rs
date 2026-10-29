//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionStack, NativeEntryState,
        driver::{ExecutionAction, ReturnAction},
        transfer::ReturnValue,
    },
    module::linked_execution::LinkedNativeBody,
    native::context::{
        CallContext, ScriptInvoker,
        arguments::{ArgumentSlots, ArgumentView},
    },
};
use kagari_bytecode::module::CallableTarget;

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
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn finish_native_return_transport(
        &self,
        runtime: &Runtime,
    ) -> Result<ReturnAction, RuntimeError> {
        self.finish_native_return(runtime)
    }

    pub(crate) fn finish_native_return(
        &self,
        runtime: &Runtime,
    ) -> Result<ReturnAction, RuntimeError> {
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

    pub(super) fn invoke_prepared_native(
        &self,
        runtime: &Runtime,
        index: usize,
        invoke_script: ScriptInvoker,
    ) -> Result<(), RuntimeError> {
        // execute_region admitted this runtime/session; no callback or collection
        // intervened. Retain the linked descriptor and roots, releasing all frame
        // and bank borrows before invoking arbitrary Rust or reentrant script.
        let (loaded, roots, links) = {
            let frame = self.current()?;
            (frame.loaded.clone(), frame.slots, frame.links.clone())
        };
        let operation = links
            .as_ref()
            .and_then(|links| links.native_call(index))
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine("missing prepared native call")
            })?;
        let LinkedNativeBody::Callback(callback) = &operation.body else {
            return Err(runtime
                .resources()
                .quarantine("sealed native body reached callback transition"));
        };
        let mut context = CallContext {
            runtime,
            owner: &loaded,
            function: &callback.function,
            invoke_script,
            arguments: ArgumentView::frame(
                &runtime.gc,
                roots,
                ArgumentSlots::Prepared(&callback.arguments),
            ),
        };
        let value = callback.function.invoke(&mut context)?;
        let mut frame = self.current_mut()?;
        if let Some(destination) = operation.destination {
            frame
                .slots
                .set_location(&runtime.gc, destination, value)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("invalid native return destination")
                })?;
        }
        frame.completed = frame.executing;
        Ok(())
    }
}
