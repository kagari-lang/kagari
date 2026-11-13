use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, SessionId},
    execution_metadata::{environments::AdmittedEnvironment, managed::sources::MetadataSource},
    frame::{ExecutionStack, FrameStack},
    native::{
        binding::NativeResult,
        context::SelectedCall,
        conversion::arguments::ArgumentValues,
        function_handle::{PreparedFunction, Target},
    },
    session::{ExecutionEntry, ExecutionOptions, ExecutionPhase, ExecutionSession},
    value::closures::ClosureObject,
    value_check,
};
use kagari_gc::error::GcError;

impl PreparedFunction {
    /// Keep only executable environment evidence in the owned invocation prefix.
    /// It must outlive factory-frame retirement and the final output converter;
    /// captures remain with their own closure/queued argument owners.
    pub(super) fn admit_owned_environment(
        &self,
        runtime: &mut Runtime,
        execution: SessionId,
    ) -> NativeResult<Option<AdmittedEnvironment>> {
        let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let environment = match &self.target {
                    Target::Entry(_) => self
                        .environment
                        .as_ref()
                        .map(|root| slots.scope().get(root))
                        .transpose()?,
                    Target::Closure { root } => {
                        let value = root.get(slots.scope())?;
                        let object = slots.scope().downcast::<ClosureObject>(
                            value.object().ok_or(GcError::Representation)?,
                        )?;
                        ClosureObject::environment(slots.scope(), object)
                    }
                };
                environment
                    .map(|environment| AdmittedEnvironment::publish(slots, execution, environment))
                    .transpose()
            })
        });
        result.map_err(|error| runtime.managed_memory_error(error))
    }

    pub(super) fn validate(&self, runtime: &mut Runtime) -> NativeResult<()> {
        runtime.with_revision_origin(&self.owner, self.body_origin(), |_, _| Ok(()))?;
        match &self.target {
            Target::Closure { root } => {
                value_check::candidate_root(runtime, root)?;
                let record = runtime
                    .memory
                    .scope(&runtime.resources, |cx, _| {
                        let value = root.get(cx)?;
                        let object = cx.downcast::<ClosureObject>(
                            value.object().ok_or(GcError::Representation)?,
                        )?;
                        Ok(cx.read(object, |object| object.record().clone()))
                    })
                    .map_err(|error| runtime.managed_memory_error(error))?;
                if record.call_route().is_some()
                    || record.implementation.program_identity() != self.owner.program_identity()
                    || record.callable_target() != self.source_entry
                {
                    return Err(RuntimeError::module_validation(
                        "prepared closure is not the exact retained computation",
                    ));
                }
            }
            Target::Entry(target) => {
                if let Some(session) = runtime.resources().active_session()
                    && session.options.phase == ExecutionPhase::CandidateInitialization
                    && session.root.program_root().key() != self.owner.program_root().key()
                {
                    return Err(RuntimeError::execution_phase_violation(
                        "external function in candidate execution",
                    ));
                }
                match (&target.environment, &self.environment) {
                    (Some(facts), Some(root)) => {
                        runtime.validate_environment_in(
                            &[MetadataSource::HostEnvironment(root)],
                            facts,
                        )?;
                    }
                    (None, None) => {}
                    _ => {
                        return Err(RuntimeError::module_validation(
                            "prepared environment ownership",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

impl Runtime {
    /// Enter a sealed, nonescaping Native-to-script selection. The caller's
    /// activation protects its origin until the frame publishes its own edge.
    pub fn enter_selected_execution_stack(
        &mut self,
        selected: &SelectedCall<'_>,
    ) -> NativeResult<ExecutionStack<'_>> {
        if self.resources().active_session().is_none() {
            return Err(RuntimeError::module_validation(
                "selected Native call has no active execution",
            ));
        }
        if self.resources().active_session().is_some_and(|session| {
            session.options.phase == ExecutionPhase::CandidateInitialization
                && session.root.program_identity() != selected.owner().program_identity()
        }) {
            return Err(RuntimeError::execution_phase_violation(
                "external function in candidate execution",
            ));
        }
        let session = self.begin_execution_inner(
            selected.owner(),
            self.execution_options(),
            ExecutionEntry::RetainedFunction,
            selected.origin(),
        )?;
        ExecutionStack::new(session)
    }

    pub(crate) fn begin_pinned_execution(
        &mut self,
        function: &PreparedFunction,
    ) -> NativeResult<ExecutionSession<'_>> {
        self.begin_pinned_execution_with_options(function, self.execution_options())
    }

    pub(crate) fn begin_pinned_execution_with_options(
        &mut self,
        function: &PreparedFunction,
        options: ExecutionOptions,
    ) -> NativeResult<ExecutionSession<'_>> {
        function.validate(self)?;
        let root_entry = self.resources().active_session().is_none();
        let mut session = self.begin_execution_inner(
            &function.owner,
            options,
            ExecutionEntry::RetainedFunction,
            function.body_origin(),
        )?;
        if root_entry {
            session.runtime_mut().attach_execution_observer()?;
        }
        Ok(session)
    }

    /// Backend entry for a checked host handle, including retained old versions.
    pub fn enter_pinned_execution_stack(
        &mut self,
        function: &PreparedFunction,
    ) -> NativeResult<ExecutionStack<'_>> {
        ExecutionStack::new(self.begin_pinned_execution(function)?)
    }
}

impl FrameStack {
    pub fn push_pinned_call(
        &self,
        runtime: &mut Runtime,
        function: &PreparedFunction,
        arguments: ArgumentValues<'_>,
    ) -> NativeResult<()> {
        function.validate(runtime)?;
        match &function.target {
            Target::Entry(target) => {
                let selected = SelectedCall::new(
                    &function.owner,
                    target,
                    function.body_origin(),
                    function
                        .environment
                        .as_ref()
                        .map(MetadataSource::HostEnvironment),
                );
                self.push_selected_call(runtime, &selected, arguments)
            }
            Target::Closure { root } => self.push_rooted_closure(runtime, root, arguments),
        }
    }
}
