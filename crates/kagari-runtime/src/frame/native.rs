//! Ordinary native functions finish synchronously using the caller's existing roots.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, NativeEntryState, driver::ExecutionAction},
    native::context::{ArgumentView, CallContext, ScriptInvoker},
};
use kagari_bytecode::module::CallableTarget;

impl ExecutionStack<'_> {
    pub fn start_native_entry(
        &self,
        runtime: &Runtime,
        invoke_script: ScriptInvoker,
    ) -> Result<ExecutionAction, RuntimeError> {
        self.validate_runtime(runtime)?;
        runtime.gc.ensure_no_native_borrow()?;
        let (loaded, import, roots, environment) = {
            let mut frames = self
                .session
                .resources
                .sessions
                .frames_mut(self.session.id)
                .ok_or_else(|| {
                    runtime
                        .resources()
                        .quarantine("native entry stack is borrowed")
                })?;
            let count = frames.len();
            let frame = frames
                .last_mut()
                .filter(|_| count > self.base)
                .ok_or_else(|| runtime.resources().quarantine("missing native entry frame"))?;
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
                &loaded
                    .execution()
                    .callee(CallableTarget::Native(import))
                    .expect("admitted Native layout")
                    .slot_layout,
                function.signature.params.len(),
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
}
