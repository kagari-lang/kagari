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
    pub target: PreparedCallTarget,
    pub arguments: Box<[Location]>,
    pub destination: Option<Location>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum PreparedCallTarget {
    Static {
        module: ModuleRef,
        target: CallableTarget,
        shared: bool,
    },
    Interface,
    Closure,
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
                let target = match *callee {
                    CallTarget::Function(function) => PreparedCallTarget::Static {
                        module: slot,
                        target: CallableTarget::Script(function),
                        shared: false,
                    },
                    CallTarget::ModuleFunction { module, function } => PreparedCallTarget::Static {
                        module,
                        target: CallableTarget::Script(function),
                        shared: false,
                    },
                    CallTarget::Shared { module, target, .. } => PreparedCallTarget::Static {
                        module,
                        target,
                        shared: true,
                    },
                    CallTarget::InterfaceMethod { .. } => PreparedCallTarget::Interface,
                    CallTarget::ClosureRegister { .. } => PreparedCallTarget::Closure,
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
                        target,
                        arguments,
                        destination,
                    },
                );
            }
        }
    }
}
