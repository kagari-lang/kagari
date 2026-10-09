//! Concrete call sites bind sealed transfers to the active caller's pinned program.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionStack, FrameDispatch, FrameEntry, arguments::FrameArguments},
};
use kagari_bytecode::module::CallableTarget;

impl ExecutionStack<'_> {
    /// Only the executing sealed instruction selects a call record; external
    /// callers cannot supply a transfer plan or substitute a target generation.
    pub fn push_prepared_call(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let (owner, function, pc, slots) = {
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
            )
        };
        let call = owner.execution().functions[function.index()]
            .calls
            .get(&pc)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine("missing prepared script call")
            })?;
        let loaded = owner.member(call.module).ok_or_else(|| {
            runtime
                .resources()
                .quarantine("invalid prepared script module")
        })?;
        self.push_admitted_arguments(
            runtime,
            loaded,
            CallableTarget::Script(call.function),
            FrameArguments::frame(slots, &call.arguments),
            None,
            FrameDispatch {
                prepared: Some(call),
                entry: FrameEntry::Call,
                interface_method: None,
                environment: None,
            },
        )
    }
}
