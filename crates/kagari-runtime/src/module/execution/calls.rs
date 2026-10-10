//! Physical argument sources from sealed call sites; callee layouts own placement.
use crate::module::execution::{ExecutionModule, layout::Location};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::{BytecodeModule, CallableTarget},
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::callable::{interface::InterfaceCallContract, witness::OperationWitness};
use kagari_types::ty::Ty;

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
    Interface {
        index: usize,
        closed: bool,
    },
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
                    CallTarget::InterfaceMethod { ref contract, .. } => {
                        let index = prepared.interface_calls;
                        prepared.interface_calls += 1;
                        let closed = closed_interface_contract(contract);
                        prepared.has_closed_interface_calls |= closed;
                        PreparedCallTarget::Interface { index, closed }
                    }
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

/// The verifier already proved each witness. This classifies lexical environment
/// dependence only; it never selects another implementation or infers a type.
fn closed_interface_contract(contract: &InterfaceCallContract<DefinitionId>) -> bool {
    contract.receiver.is_none()
        && Ty::Trait(contract.interface.clone()).is_concrete()
        && contract.arguments.iter().all(Ty::is_concrete)
        && contract.operations.iter().all(|operation| match operation {
            OperationWitness::Selected(_) => true,
            OperationWitness::Forward(_) => false,
            OperationWitness::SharedMethod(selected) => {
                selected
                    .implementation
                    .arguments
                    .iter()
                    .all(Ty::is_concrete)
                    && selected.requirement.receiver.is_concrete()
                    && Ty::Trait(selected.requirement.interface.clone()).is_concrete()
                    && selected.requirement.arguments.iter().all(Ty::is_concrete)
            }
        })
}
