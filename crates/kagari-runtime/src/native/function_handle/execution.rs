use crate::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionStack,
    native::{
        binding::NativeResult,
        function_handle::{PreparedFunction, Target},
    },
    session::{ExecutionEntry, ExecutionOptions, ExecutionPhase, ExecutionSession},
    value::Value,
};

impl PreparedFunction {
    pub(super) fn validate(&self, runtime: &Runtime) -> NativeResult<()> {
        runtime.gc().ensure_no_storage_borrow()?;
        runtime.resources().ensure_execution_allowed()?;
        runtime.validate_executable_module(&self.owner)?;
        match &self.target {
            Target::Closure { prepared, root } => {
                if root.value(runtime.gc()).as_ref() != Some(prepared.value()) {
                    return Err(RuntimeError::module_validation(
                        "foreign or expired function handle",
                    ));
                }
                prepared.validate(runtime)?;
            }
            Target::Entry(_) => {
                if let Some(session) = runtime.resources().active_session()
                    && session.options.phase == ExecutionPhase::CandidateInitialization
                    && session.root.program_root().key() != self.owner.program_root().key()
                {
                    return Err(RuntimeError::execution_phase_violation(
                        "external function in candidate execution",
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Runtime {
    pub(crate) fn begin_pinned_execution(
        &self,
        function: &PreparedFunction,
    ) -> NativeResult<ExecutionSession<'_>> {
        self.begin_pinned_execution_with_options(function, self.execution_options())
    }

    pub(crate) fn begin_pinned_execution_with_options(
        &self,
        function: &PreparedFunction,
        options: ExecutionOptions,
    ) -> NativeResult<ExecutionSession<'_>> {
        function.validate(self)?;
        let root_entry = self.resources().active_session().is_none();
        let session =
            self.begin_execution_inner(&function.owner, options, ExecutionEntry::RetainedFunction)?;
        if root_entry {
            self.attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Backend entry for a checked host handle, including retained old versions.
    pub fn enter_pinned_execution_stack(
        &self,
        function: &PreparedFunction,
    ) -> NativeResult<ExecutionStack<'_>> {
        ExecutionStack::new(self.begin_pinned_execution(function)?)
    }
}

impl ExecutionStack<'_> {
    /// Access backend execution transport through its original semantic owner.
    ///
    /// # Safety
    /// Keep every managed input in this runtime's roots or active traced storage
    /// until publication. Protect extracted results before collection, callbacks
    /// or mutation can remove their last owner. Returned loans must end before
    /// reentry; scoped host resources also require their original host lifetime.
    /// This entry preserves validation but cannot revive expired direct references.
    #[doc(hidden)]
    pub unsafe fn push_pinned_call_transport(
        &self,
        runtime: &Runtime,
        function: &PreparedFunction,
        arguments: &[Value],
    ) -> NativeResult<()> {
        self.push_pinned_call(runtime, function, arguments)
    }

    pub(crate) fn push_pinned_call(
        &self,
        runtime: &Runtime,
        function: &PreparedFunction,
        arguments: &[Value],
    ) -> NativeResult<()> {
        function.validate(runtime)?;
        match &function.target {
            Target::Entry(target) => {
                self.push_selected_call(runtime, &function.owner, target, arguments)
            }
            Target::Closure { prepared, .. } => {
                self.push_closure(runtime, prepared.value(), arguments, None)
            }
        }
    }
}
