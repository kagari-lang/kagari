//! Cold script construction and resume transfer the existing graph into frame storage.
use crate::{
    Runtime,
    closure::{ClosureMode, ClosureRecord, ClosureTarget},
    error::RuntimeError,
    execution_metadata::managed::sources::MetadataSource,
    frame::{
        FrameDispatch, FrameEntry, FrameEnvironment, FrameStack, arguments::FrameArguments,
        closures::closure_inputs, types::arguments::TypeArgument,
    },
    module::{ExecutableModule, managed::source::RevisionSource},
    native::{
        binding::NativeResult,
        conversion::context::ConversionContext,
        future::{ColdFuture, claims::ClaimedFuture, storage::ColdObject},
    },
    session::ExecutionPhase,
    value::{closures::ClosureObject, place::ResultPlace},
};
use kagari_bytecode::instruction::Slot;
use kagari_contract::ids::FunctionRef;
use kagari_gc::error::GcError;
use kagari_types::ty::Ty;
use std::{slice, sync::Arc};

impl FrameStack {
    pub(crate) fn make_future(
        &self,
        runtime: &mut Runtime,
        function: FunctionRef,
        captures: &[Slot],
        destination: Slot,
        future: &Ty,
    ) -> NativeResult<()> {
        self.validate_top(runtime)?;
        runtime.reject_static_initialization("cold Future construction")?;
        if runtime.execution_options().phase != ExecutionPhase::Ordinary {
            return Err(RuntimeError::execution_phase_violation(
                "cold Future construction",
            ));
        }
        let (owner, environment, slots, destination, locations) = {
            let frame = self.current(runtime)?;
            let destination = frame.slot_location(runtime.resources(), destination, false)?;
            let mut locations = Vec::new();
            locations
                .try_reserve_exact(captures.len())
                .map_err(|_| RuntimeError::resource_limit("Future capture locations"))?;
            for register in captures {
                locations.push(frame.slot_location(runtime.resources(), *register, true)?);
            }
            (
                frame.loaded.clone(),
                frame.admitted_environment().cloned(),
                frame.slots(),
                destination,
                locations,
            )
        };
        let target = owner
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(invalid)?;
        if !target.metadata.effects.may_suspend || target.metadata.params.len() != captures.len() {
            return Err(invalid());
        }
        let mut values = Vec::new();
        values
            .try_reserve_exact(locations.len())
            .map_err(|_| RuntimeError::resource_limit("Future capture views"))?;
        for (index, location) in locations.into_iter().enumerate() {
            if index.is_multiple_of(1024) {
                runtime.resources.poll_execution()?;
            }
            values.push(
                slots
                    .inspect(&runtime.memory, location)
                    .map_err(|error| runtime.managed_memory_error(error))?,
            );
        }
        let future = runtime
            .type_arguments(
                &owner,
                environment.as_ref().map(|env| env.facts().types.clone()),
                slice::from_ref(future),
            )?
            .pop()
            .ok_or_else(invalid)?;
        let record = Arc::new(ClosureRecord::script(
            runtime,
            &owner,
            function,
            values.len(),
            environment.as_ref().map(|env| env.facts().clone()),
            ClosureMode::Retained,
        )?);
        let output = future.parameter(runtime, &owner, 0)?;
        if !output
            .view(&owner)
            .compatible(record.signature().result.view(&owner))
        {
            return Err(invalid());
        }
        {
            let mut conversion = ConversionContext::for_selected(
                runtime,
                &owner,
                self.session,
                RevisionSource::Execution(slots.body),
                environment
                    .as_ref()
                    .map(MetadataSource::AdmittedEnvironment),
            )?;
            let closure = conversion.publish_closure(record, &values)?;
            let value = conversion.allocate_future(
                &future,
                ColdFuture::Script {
                    owner: owner.clone(),
                },
                &[closure],
            )?;
            ResultPlace::frame(&slots, Some(&destination))
                .publish(conversion.runtime_mut(), &value)?
                .admit_frame(&slots, Some(&destination))?;
        }
        self.complete_source_write(runtime);
        Ok(())
    }

    pub(super) fn push_future(
        &self,
        runtime: &mut Runtime,
        claim: ClaimedFuture,
        destination: Option<Slot>,
        output: &TypeArgument,
    ) -> NativeResult<()> {
        self.validate_top(runtime)?;
        if !self.can_park(runtime)? {
            return Err(invalid());
        }
        let ColdFuture::Script { owner } = claim.recipe() else {
            return Err(invalid());
        };
        let owner = owner.clone();
        let closure = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let cold = cx.get(claim.source())?;
                let value = ColdObject::capture(cx, cold, 0)?;
                let closure =
                    cx.downcast::<ClosureObject>(value.object().ok_or(GcError::Representation)?)?;
                Ok(cx.read(closure, |closure| closure.record().clone()))
            })
            .map_err(|error| runtime.managed_memory_error(error))?;
        runtime.validate_future_resume(&closure, &owner, output)?;
        let arguments = FrameArguments::plain(&[])
            .with_cold_captures(claim.source(), closure.capture_types.len())
            .map_err(|error| runtime.managed_memory_error(error))?;
        closure_inputs(runtime, &closure, arguments, 0)?;
        let environment = closure.environment.as_ref().map(|facts| FrameEnvironment {
            facts: facts.clone(),
            source: MetadataSource::ColdClosureEnvironment(claim.source()),
        });
        self.push_admitted_arguments(
            runtime,
            owner,
            closure.callable_target(),
            arguments,
            destination,
            FrameDispatch {
                origin: RevisionSource::Cold(claim.source()),
                metadata: Some(MetadataSource::Cold(claim.source())),
                prepared: None,
                entry: FrameEntry::Await,
                invocation: None,
                environment,
            },
        )?;
        // The new frame owns all captures and executable dependencies now. There
        // is no floating conversion suffix to retire across this live frame.
        Ok(())
    }
}

impl Runtime {
    pub(crate) fn validate_future_resume(
        &self,
        closure: &Arc<ClosureRecord>,
        owner: &ExecutableModule,
        output: &TypeArgument,
    ) -> NativeResult<()> {
        let ClosureTarget::Script(function) = closure.target else {
            return Err(invalid());
        };
        self.validate_executable_module(&closure.implementation)?;
        if closure.implementation.program_identity() != owner.program_identity() {
            return Err(invalid());
        }
        let target = closure
            .implementation
            .bytecode
            .functions
            .get(function.index())
            .ok_or_else(invalid)?;
        if !target.metadata.effects.may_suspend
            || target.metadata.params.len() != closure.capture_types.len()
            || !closure.signature().params.is_empty()
            || !output
                .view(owner)
                .compatible(closure.signature().result.view(&closure.implementation))
        {
            return Err(invalid());
        }
        Ok(())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("cold script Future contract")
}
