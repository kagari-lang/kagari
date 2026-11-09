//! Call contracts use the loaded definition scope, unlike shared physical code.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{MetadataEdge, call_contracts::ScopedInterfaceCall},
    module::{
        ExecutableModule,
        dispatch::{
            CallRoute,
            bindings::{PreparedSelection, PublishedRoutes},
        },
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

#[derive(Debug, Clone)]
pub(crate) enum LinkedCall {
    Physical,
    Direct(BoundRoute),
    Shared {
        route: BoundRoute,
        contract: Box<SharedCall>,
    },
    Interface {
        contract: Box<InterfaceCallContract>,
        prepared: Option<Arc<ScopedInterfaceCall>>,
    },
    Closure(Option<Box<Ty>>),
}

#[derive(Debug, Clone)]
pub(crate) struct BoundRoute {
    route: CallRoute,
    pub(crate) selected: Arc<PreparedSelection>,
}

impl BoundRoute {
    fn new(route: CallRoute) -> Self {
        Self {
            selected: route.initial_selection(),
            route,
        }
    }
}

impl LinkedCall {
    pub(super) fn rebound(&self, routes: &PublishedRoutes) -> Result<Self, RuntimeError> {
        let mut result = self.clone();
        let binding = match &mut result {
            Self::Direct(binding) | Self::Shared { route: binding, .. } => binding,
            _ => return Ok(result),
        };
        binding.selected = routes.selection(&binding.route)?;
        Ok(result)
    }

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
        owner: &ExecutableModule,
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
                    (
                        PreparedCallTarget::Shared { module, target },
                        CallTarget::Shared { contract, .. },
                    ) => Ok(LinkedCall::Shared {
                        route: BoundRoute::new(
                            owner
                                .member(*module)
                                .ok_or_else(invalid)?
                                .bind_call_route(*target)?,
                        ),
                        contract: contract.clone(),
                    }),
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
                                .slots
                                .get(&register.index())
                                .cloned()
                                .map(Box::new),
                        ))
                    }
                    (
                        PreparedCallTarget::Static { module, target },
                        CallTarget::Function(_)
                        | CallTarget::ModuleFunction { .. }
                        | CallTarget::Native(_),
                    ) => Ok(LinkedCall::Direct(BoundRoute::new(
                        owner
                            .member(*module)
                            .ok_or_else(invalid)?
                            .bind_call_route(*target)?,
                    ))),
                    (PreparedCallTarget::Closure(_), CallTarget::ClosureRegister { .. }) => {
                        Ok(LinkedCall::Physical)
                    }
                    _ => Err(invalid()),
                }
            })
            .collect()
    }
}
