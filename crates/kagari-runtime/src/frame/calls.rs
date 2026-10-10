//! Statically selected call sites share physical argument admission and publication.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments},
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::CallableTarget,
};

impl ExecutionStack<'_> {
    /// Only the executing sealed instruction selects a call record; external
    /// callers cannot supply a transfer plan or substitute a target generation.
    pub fn push_prepared_call(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let (owner, function, pc, slots, environment) = {
            let frames = self.frames()?;
            let caller = frames
                .last()
                .filter(|_| frames.len() > self.base)
                .ok_or_else(|| runtime.resources().quarantine("missing execution frame"))?;
            let CallableTarget::Script(function) = caller.target else {
                return Err(runtime
                    .resources()
                    .quarantine("script call requires a script caller"));
            };
            (
                caller.loaded.clone(),
                function,
                caller.instruction_offset(),
                caller.slots,
                caller.environment(),
            )
        };
        let call = owner.execution().functions[function.index()]
            .calls
            .get(&pc)
            .ok_or_else(|| runtime.resources().quarantine("missing prepared call"))?;
        let loaded = owner.member(call.module).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid prepared script module")
        })?;
        let arguments = FrameArguments::frame(slots, &call.arguments);
        let environment = if call.shared {
            let BytecodeInstruction::Call {
                callee: CallTarget::Shared { contract, .. },
                ..
            } = &owner.bytecode.functions[function.index()].instructions[pc]
            else {
                return Err(runtime
                    .resources()
                    .quarantine("missing prepared shared contract"));
            };
            let prepared = runtime.prepare_shared_environment(
                &owner,
                environment.clone(),
                &loaded,
                call.target,
                contract,
            )?;
            if arguments.len() != contract.signature.params.len()
                || !arguments.all(runtime, |index, value| {
                    runtime.matches_type_in(
                        value,
                        &contract.signature.params[index],
                        &owner,
                        environment
                            .as_ref()
                            .map(|environment| environment.types.as_ref()),
                    )
                })?
            {
                return Err(RuntimeError::module_validation("invalid shared call entry"));
            }
            Some(prepared)
        } else {
            None
        };
        self.push_admitted_arguments(
            runtime,
            loaded,
            call.target,
            arguments,
            None,
            FrameDispatch {
                prepared: Some(call),
                entry: FrameEntry::Call,
                interface_method: None,
                environment,
            },
        )
    }
}
