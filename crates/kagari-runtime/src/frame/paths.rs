//! Host path instructions snapshot operands before callbacks and commit once.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    frame::{FrameStack, instruction::Inputs},
    host::{HostCallContext, HostPathDescriptorId},
};
use kagari_bytecode::instruction::{BinaryOp, Slot};

#[derive(Clone, Copy)]
pub(crate) enum PathAction {
    Read(Slot),
    MakeView(Slot),
    Set(Slot),
    Modify {
        destination: Option<Slot>,
        value: Slot,
        operation: BinaryOp,
    },
}

impl PathAction {
    fn output(&self) -> Option<Slot> {
        match *self {
            Self::Read(slot) | Self::MakeView(slot) => Some(slot),
            Self::Modify { destination, .. } => destination,
            Self::Set(_) => None,
        }
    }

    fn input(&self) -> Option<Slot> {
        match *self {
            Self::Set(slot) | Self::Modify { value: slot, .. } => Some(slot),
            Self::Read(_) | Self::MakeView(_) => None,
        }
    }
}

impl FrameStack {
    pub(crate) fn access_host_path(
        &self,
        runtime: &mut Runtime,
        descriptor: HostPathDescriptorId,
        root: Slot,
        arguments: &[Slot],
        action: PathAction,
    ) -> Result<(), RuntimeError> {
        runtime.reject_candidate_external_access()?;
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(
                arguments
                    .len()
                    .checked_add(2)
                    .ok_or_else(|| RuntimeError::resource_limit("path operand count"))?,
            )
            .map_err(|_| RuntimeError::resource_limit("path operand count"))?;
        fields.push(root);
        fields.extend_from_slice(arguments);
        fields.extend(action.input());
        let inputs = match action.output() {
            Some(output) => Inputs::prepare(self, runtime, &fields, output)?,
            None => Inputs::prepare_effect(self, runtime, &fields)?,
        };
        let views = inputs.views(runtime)?;
        let mut call = HostCallContext::new(runtime, &[])?;
        let (root, args, value) = call.snapshot_path_inputs(
            &views[0],
            &views[1..1 + arguments.len()],
            action.input().map(|_| &views[arguments.len() + 1]),
        )?;
        let result = match action {
            PathAction::Read(_) => call.read_path(&root, descriptor, &args).map(Some),
            PathAction::MakeView(_) => call.make_path_view(&root, descriptor, &args).map(Some),
            PathAction::Set(_) => call
                .set_path(&root, descriptor, &args, &value.expect("set input"))
                .map(|()| None),
            PathAction::Modify { operation, .. } => call
                .modify_path(
                    &root,
                    descriptor,
                    &args,
                    operation,
                    &value.expect("modify input"),
                )
                .map(Some),
        };
        // Callback termination precedes its business outcome, with all callback
        // and physical loans ended. Completed Host effects are never replayed.
        call.runtime().resources.ensure_execution_allowed()?;
        let value = result?;
        match (action.output(), value) {
            (Some(destination), Some(value)) => {
                self.write_slot(call.runtime_mut(), destination, &value)
            }
            (None, _) => {
                self.complete_source_write(call.runtime());
                Ok(())
            }
            (Some(_), None) => Err(call
                .runtime()
                .resources
                .quarantine(EngineFault::HostInvariant("Host path lost its result"))),
        }
    }
}
