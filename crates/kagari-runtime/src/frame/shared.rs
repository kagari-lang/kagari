//! Admit an exact callback or host selection without selecting its route again.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments},
    module::ExecutableModule,
    native::context::LinkedCallable,
    value::Value,
};

impl ExecutionStack<'_> {
    /// Admit an exact callback/host selection. Its producer owns current-route
    /// selection and environment transport; retained queued work must not reselect.
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn push_selected_call_transport(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
        selected: &LinkedCallable,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        self.push_selected_call(runtime, caller, selected, args)
    }

    pub(crate) fn push_selected_call(
        &self,
        runtime: &Runtime,
        caller: &ExecutableModule,
        selected: &LinkedCallable,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        let owner = selected.owner(caller)?;
        runtime.validate_executable_module(&owner)?;
        if args.len() != selected.params.len()
            || !args
                .iter()
                .enumerate()
                .all(|(index, value)| selected.matches_argument(runtime, &owner, index, value))
        {
            return Err(RuntimeError::module_validation("selected call arguments"));
        }
        self.push_arguments(
            runtime,
            owner,
            selected.target,
            FrameArguments::plain(args),
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
