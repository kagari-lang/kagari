//! Admit an exact callback or host selection without selecting its route again.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, FrameDispatch, FrameEntry, arguments::InvocationArguments},
    module::ExecutableModule,
    native::context::LinkedCallable,
};

impl ExecutionStack<'_> {
    /// Admit an exact callback/host selection. Its producer owns current-route
    /// selection and environment transport; retained queued work must not reselect.
    pub fn push_selected_call(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
        selected: &LinkedCallable,
        args: InvocationArguments<'_>,
    ) -> Result<(), RuntimeError> {
        let owner = selected.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
        if args.len() != selected.params.len()
            || !args.source.all(runtime, |index, value| {
                selected.matches_argument(runtime, &owner, index, value)
            })?
        {
            return Err(RuntimeError::module_validation("selected call arguments"));
        }
        self.push_arguments(
            runtime,
            owner,
            selected.target,
            args.source,
            None,
            FrameDispatch {
                prepared: None,
                entry: FrameEntry::Call,
                invocation: None,
                environment: selected.environment.clone(),
            },
        )
    }
}
