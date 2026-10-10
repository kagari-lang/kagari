//! Physical argument sources from sealed call sites; callee layouts own placement.
use crate::module::execution::{ExecutionModule, layout::Location};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::{BytecodeModule, CallableTarget},
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionId;

#[derive(Debug)]
pub(crate) struct PreparedCall {
    pub module: ModuleRef,
    pub target: CallableTarget,
    pub shared: bool,
    pub arguments: Box<[Location]>,
    pub destination: Option<Location>,
}

impl ExecutionModule {
    /// Module slots refer to the containing verified program. Activation binds
    /// them through the caller's pinned descriptor, never the latest publication.
    pub(crate) fn prepare_calls(&mut self, slot: ModuleRef, module: &BytecodeModule<DefinitionId>) {
        for (prepared, function) in self.functions.iter_mut().zip(&module.functions) {
            for (pc, instruction) in function.instructions.iter().enumerate() {
                let BytecodeInstruction::Call { dst, callee, args } = instruction else {
                    continue;
                };
                let (module, target) = match *callee {
                    CallTarget::Function(function) => (slot, CallableTarget::Script(function)),
                    CallTarget::ModuleFunction { module, function } => {
                        (module, CallableTarget::Script(function))
                    }
                    CallTarget::Shared { module, target, .. } => (module, target),
                    _ => continue,
                };
                let arguments = args
                    .iter()
                    .map(|register| {
                        prepared
                            .registers
                            .location(register.index())
                            .expect("sealed caller operand")
                    })
                    .collect();
                let destination = dst.map(|register| {
                    prepared
                        .registers
                        .location(register.index())
                        .expect("sealed return operand")
                });
                prepared.calls.insert(
                    pc,
                    PreparedCall {
                        module,
                        target,
                        shared: matches!(callee, CallTarget::Shared { .. }),
                        arguments,
                        destination,
                    },
                );
            }
        }
    }
}
