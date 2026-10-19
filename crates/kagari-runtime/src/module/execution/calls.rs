//! Physical argument sources from sealed call sites; callee layouts own placement.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::{self, Event};

use crate::module::execution::{
    ExecutionInstruction, ExecutionModule, layout::Location, managed::ManagedOperation,
};
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget, Slot},
    module::CallableTarget,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_common::identity::table::DefinitionTable;
use kagari_contract::callable::{interface::InterfaceCallContract, witness::OperationWitness};
use kagari_types::ty::Ty;

#[derive(Debug)]
pub(crate) struct PreparedCall {
    pub pc: usize,
    pub target: PreparedCallTarget,
    pub arguments: Box<[Location]>,
    pub destination: Option<Location>,
}

#[derive(Debug)]
pub(crate) enum PreparedCallTarget {
    Static {
        module: ModuleRef,
        target: CallableTarget,
    },
    Shared {
        module: ModuleRef,
        target: CallableTarget,
    },
    Interface {
        closed: bool,
        receiver: Option<Slot>,
    },
    Closure(Box<PreparedClosureCall>),
}

/// A closure's verified physical contract or scoped-signature marker.
/// Runtime links supply lexical types; no active value/window is retained here.
#[derive(Debug)]
pub(crate) struct PreparedClosureCall {
    pub register: Slot,
    pub signature: PreparedClosureSignature,
}

#[derive(Debug)]
pub(crate) enum PreparedClosureSignature {
    Physical {
        params: Box<[ValueType]>,
        result: ValueType,
    },
    Scoped,
}

impl ExecutionModule {
    /// Module slots refer to the containing verified program. Activation binds
    /// them through the caller's pinned descriptor, never the latest publication.
    pub(crate) fn prepare_calls(
        &mut self,
        slot: ModuleRef,
        program: &BytecodeProgram,
        definitions: &DefinitionTable,
    ) {
        let module = &program.modules[slot.index()];
        for (prepared, function) in self.functions.iter_mut().zip(&module.functions) {
            let mut calls = Vec::new();
            for (pc, instruction) in function.instructions.iter().enumerate() {
                let BytecodeInstruction::Call { dst, callee, args } = instruction else {
                    continue;
                };
                let target = match *callee {
                    CallTarget::Function(function) => PreparedCallTarget::Static {
                        module: slot,
                        target: CallableTarget::Script(function),
                    },
                    CallTarget::ModuleFunction { module, function } => PreparedCallTarget::Static {
                        module,
                        target: CallableTarget::Script(function),
                    },
                    CallTarget::Shared { module, target, .. } => {
                        prepared.has_call_contracts = true;
                        PreparedCallTarget::Shared { module, target }
                    }
                    CallTarget::InterfaceMethod {
                        module,
                        ref contract,
                    } => {
                        let owner = &program.modules[module.index()];
                        let signature = contract
                            .signature_in(
                                &owner.identity,
                                &owner.public_items,
                                &owner.trait_contracts,
                                &Default::default(),
                                definitions,
                            )
                            .expect("verified interface signature");
                        let closed = closed_interface_contract(contract);
                        prepared.has_call_contracts = true;
                        PreparedCallTarget::Interface {
                            closed,
                            receiver: signature
                                .has_receiver
                                .then(|| *args.first().expect("verified interface receiver")),
                        }
                    }
                    CallTarget::ClosureRegister {
                        register,
                        ref params,
                        return_type,
                    } => {
                        let signature = if return_type == ValueType::Generic
                            || params.contains(&ValueType::Generic)
                        {
                            prepared.has_call_contracts = true;
                            PreparedClosureSignature::Scoped
                        } else {
                            PreparedClosureSignature::Physical {
                                params: params.clone().into_boxed_slice(),
                                result: return_type,
                            }
                        };
                        PreparedCallTarget::Closure(Box::new(PreparedClosureCall {
                            register,
                            signature,
                        }))
                    }
                    _ => continue,
                };
                let arguments = args
                    .iter()
                    .map(|register| {
                        prepared
                            .frame
                            .slot_layout
                            .location(register.index())
                            .expect("sealed caller operand")
                    })
                    .collect();
                let destination = dst.map(|register| {
                    prepared
                        .frame
                        .slot_layout
                        .location(register.index())
                        .expect("sealed return operand")
                });
                #[cfg(feature = "execution-diagnostics")]
                diagnostics::record(Event::CallDescriptor);
                prepared.instructions[pc] =
                    ExecutionInstruction::managed(ManagedOperation::Call { index: calls.len() });
                calls.push(PreparedCall {
                    pc,
                    target,
                    arguments,
                    destination,
                });
            }
            prepared.calls = calls.into_boxed_slice();
        }
    }
}

/// The verifier already proved each witness. This classifies lexical environment
/// dependence only; it never selects another implementation or infers a type.
fn closed_interface_contract(contract: &InterfaceCallContract) -> bool {
    contract.receiver.is_none()
        && Ty::Trait(contract.interface.clone()).is_concrete()
        && contract.arguments.iter().all(Ty::is_concrete)
        && contract
            .operations
            .iter()
            .flat_map(OperationWitness::walk)
            .all(|operation| match operation {
                OperationWitness::Selected(_) => true,
                OperationWitness::Forward(_) => false,
                OperationWitness::SharedAdapter(adapter) => {
                    adapter.call.arguments.iter().all(Ty::is_concrete)
                }
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
