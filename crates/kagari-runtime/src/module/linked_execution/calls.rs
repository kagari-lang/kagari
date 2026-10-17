//! Call contracts use the loaded definition scope, unlike shared physical code.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{MetadataEdge, call_contracts::ScopedInterfaceCall},
    module::{
        LoadedModule,
        execution::{
            ExecutionFunction,
            calls::{PreparedCallTarget, PreparedClosureSignature},
        },
    },
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeFunction,
};
use kagari_contract::callable::{interface::InterfaceCallContract, shared::SharedCall};
use kagari_types::ty::Ty;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) enum LinkedCall {
    Physical,
    Shared(Box<SharedCall>),
    Interface {
        contract: Box<InterfaceCallContract>,
        prepared: Option<Arc<ScopedInterfaceCall>>,
    },
    Closure(Option<Box<Ty>>),
}

impl LinkedCall {
    pub(super) fn interface(&self) -> Option<&Arc<ScopedInterfaceCall>> {
        match self {
            Self::Interface { prepared, .. } => prepared.as_ref(),
            _ => None,
        }
    }
}

impl Runtime {
    pub(super) fn link_calls(
        &self,
        owner: &LoadedModule,
        function: &BytecodeFunction,
        prepared: &ExecutionFunction,
    ) -> Result<Arc<[LinkedCall]>, RuntimeError> {
        prepared
            .calls
            .iter()
            .map(|call| {
                let invalid = || RuntimeError::module_validation("invalid prepared call contract");
                let BytecodeInstruction::Call { callee, .. } =
                    function.instructions.get(call.pc).ok_or_else(invalid)?
                else {
                    return Err(invalid());
                };
                match (&call.target, callee) {
                    (PreparedCallTarget::Shared { .. }, CallTarget::Shared { contract, .. }) => {
                        Ok(LinkedCall::Shared(contract.clone()))
                    }
                    (
                        PreparedCallTarget::Interface { closed, .. },
                        CallTarget::InterfaceMethod { contract, .. },
                    ) => {
                        let prepared = if *closed {
                            let call = self.build_interface_call(owner, None, contract)?;
                            let dependencies =
                                self.metadata_dependencies(MetadataEdge::InterfaceCall(&call))?;
                            if dependencies.iter().any(|dependency| {
                                dependency.program_identity() != owner.program_identity()
                            }) {
                                return Err(RuntimeError::module_validation(
                                    "foreign closed call dependency",
                                ));
                            }
                            Some(call)
                        } else {
                            None
                        };
                        Ok(LinkedCall::Interface {
                            contract: contract.clone(),
                            prepared,
                        })
                    }
                    (
                        PreparedCallTarget::Closure(closure),
                        CallTarget::ClosureRegister { register, .. },
                    ) if matches!(&closure.signature, PreparedClosureSignature::Scoped) => {
                        Ok(LinkedCall::Closure(
                            function
                                .metadata
                                .semantic
                                .registers
                                .get(&register.index())
                                .cloned()
                                .map(Box::new),
                        ))
                    }
                    (
                        PreparedCallTarget::Static { .. },
                        CallTarget::Function(_) | CallTarget::ModuleFunction { .. },
                    )
                    | (PreparedCallTarget::Closure(_), CallTarget::ClosureRegister { .. }) => {
                        Ok(LinkedCall::Physical)
                    }
                    _ => Err(invalid()),
                }
            })
            .collect()
    }
}
