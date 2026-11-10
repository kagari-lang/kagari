//! Prepared interface conversions publish their result after cursor borrows end.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::ExecutionStack,
};

use kagari_bytecode::module::CallableTarget;

impl ExecutionStack<'_> {
    pub(super) fn execute_interface_upcast(
        &self,
        runtime: &Runtime,
        index: usize,
    ) -> Result<(), RuntimeError> {
        let invalid = || {
            runtime
                .resources()
                .quarantine(EngineFault::InvalidPreparedInterfaceUpcast)
        };
        let (owner, target, links, environment, slots) = {
            let frame = self.current()?;
            (
                frame.loaded.clone(),
                frame.target,
                frame.links.clone(),
                frame.environment(),
                frame.slots,
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
            environment.as_ref(),
            links.applied_upcasts.as_deref(),
            index,
            operand,
        )?;
        let value = runtime
            .resources()
            .frame_values
            .try_borrow()
            .map_err(|_| invalid())?
            .with_location(slots, operand.value, |value| *value)
            .ok_or_else(invalid)?;
        // The sealed instruction establishes the destination type. Receiver,
        // parent relation and live handle checks remain at the value boundary.
        let value = runtime.upcast_interface(&value, closed.source(), closed.target())?;
        if !runtime.gc().validate_value(&value) {
            return Err(invalid());
        }
        runtime
            .resources()
            .frame_values
            .try_borrow_mut()
            .map_err(|_| invalid())?
            .set_location(slots, operand.dst, value)
            .ok_or_else(invalid)?;
        let mut frame = self.current_mut()?;
        frame.completed = frame.executing;
        Ok(())
    }
}
