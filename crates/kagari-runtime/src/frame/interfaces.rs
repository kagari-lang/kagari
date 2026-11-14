//! Prepared interface conversions publish their result after cursor borrows end.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution_metadata::managed::sources::MetadataSource,
    frame::FrameStack,
    module::managed::source::RevisionSource,
    native::conversion::context::ConversionContext,
    value::place::ResultPlace,
};

use kagari_bytecode::{
    instruction::{ConstructionTarget, Slot},
    module::CallableTarget,
};

impl FrameStack {
    pub(crate) fn make_interface(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        input: Slot,
        target: &ConstructionTarget,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        let (owner, environment, slots, input, destination) = {
            let frame = self.current(runtime)?;
            (
                frame.loaded.clone(),
                frame.admitted_environment().cloned(),
                frame.slots(),
                frame.slot_location(runtime.resources(), input, true)?,
                frame.slot_location(runtime.resources(), destination, false)?,
            )
        };
        let input = slots
            .inspect(&runtime.memory, input)
            .map_err(|error| runtime.managed_memory_error(error))?;
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
            let value = match target {
                ConstructionTarget::Declared { .. } => conversion.make_declared_interface(
                    target,
                    environment.as_ref().map(|env| env.facts().clone()),
                    &input,
                )?,
                ConstructionTarget::Bound(required) => {
                    let environment = environment.as_ref().ok_or_else(|| {
                        RuntimeError::from(EngineFault::HostInvariant(
                            "missing construction environment",
                        ))
                    })?;
                    conversion.make_bound_interface(environment.facts(), required, &input)?
                }
            };
            ResultPlace::frame(&slots, Some(&destination))
                .publish(conversion.runtime_mut(), &value)?
                .admit_frame(&slots, Some(&destination))?;
        }
        self.complete_source_write(runtime);
        Ok(())
    }

    pub(super) fn execute_interface_upcast(
        &self,
        runtime: &mut Runtime,
        index: usize,
    ) -> Result<(), RuntimeError> {
        let invalid = || {
            runtime
                .resources()
                .quarantine(EngineFault::InvalidPreparedInterfaceUpcast)
        };
        let (owner, target, links, environment, slots) = {
            let frame = self.current(runtime)?;
            (
                frame.loaded.clone(),
                frame.target,
                frame.links.clone(),
                frame.admitted_environment().cloned(),
                frame.slots(),
            )
        };
        let CallableTarget::Script(function) = target else {
            return Err(invalid());
        };
        let operand = owner
            .execution()
            .functions
            .get(function.index())
            .and_then(|function| function.upcasts.get(index))
            .ok_or_else(invalid)?;
        let links = links.as_ref().ok_or_else(invalid)?;
        let bindings = links.upcasts.as_ref().ok_or_else(invalid)?;
        let closed = bindings.resolve(
            runtime,
            &owner,
            environment.as_ref().map(|env| env.facts()),
            links.applied_upcasts.as_deref(),
            index,
            operand,
        )?;
        let value = slots
            .inspect(&runtime.memory, operand.value)
            .map_err(|error| runtime.managed_memory_error(error))?;
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
            let value = conversion.upcast_interface(&value, closed.source(), closed.target())?;
            ResultPlace::frame(&slots, Some(&operand.dst))
                .publish(conversion.runtime_mut(), &value)?
                .admit_frame(&slots, Some(&operand.dst))?;
        }
        self.complete_source_write(runtime);
        Ok(())
    }
}
