//! Call contracts use the loaded definition scope, unlike shared physical code.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::SessionId,
    execution_metadata::{call_contracts::ScopedInterfaceCall, managed::sources::MetadataSource},
    module::{
        ExecutableModule, ModuleKey,
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
    module::{BytecodeFunction, CallableTarget},
};
use kagari_contract::{
    callable::{interface::InterfaceCallContract, shared::SharedCall},
    ids::FunctionRef,
};
use kagari_types::ty::Ty;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) enum LinkedCall {
    Physical,
    Direct(BoundRoute),
    SelfTail(Box<TailLink>),
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

/// Only candidate sites carry these extra facts. Ordinary direct bindings keep
/// their existing layout. Published route refresh computes eligibility once.
#[derive(Debug, Clone)]
pub(crate) struct TailLink {
    binding: BoundRoute,
    owner: ModuleKey,
    function: FunctionRef,
    same_body: bool,
}

impl TailLink {
    fn new(binding: BoundRoute, owner: ModuleKey, function: FunctionRef) -> Self {
        let same_body = binding
            .selected
            .selects(owner, CallableTarget::Script(function));
        Self {
            binding,
            owner,
            function,
            same_body,
        }
    }
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
            Self::SelfTail(tail) => &mut tail.binding,
            _ => return Ok(result),
        };
        binding.selected = routes.selection(&binding.route)?;
        if let Self::SelfTail(tail) = &mut result {
            tail.same_body = tail
                .binding
                .selected
                .selects(tail.owner, CallableTarget::Script(tail.function));
        }
        Ok(result)
    }

    pub(crate) fn direct(&self) -> Option<&BoundRoute> {
        match self {
            Self::Direct(binding) => Some(binding),
            Self::SelfTail(tail) => Some(&tail.binding),
            _ => None,
        }
    }

    pub(crate) fn reuses_self(&self) -> bool {
        matches!(self, Self::SelfTail(tail) if tail.same_body)
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
        &mut self,
        execution: SessionId,
        owner: &ExecutableModule,
        function: &BytecodeFunction,
        prepared: &ExecutionFunction,
        protected: &mut Vec<MetadataSource<'_>>,
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
                            let call = self.build_interface_call(
                                execution, owner, None, contract, protected,
                            )?;
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
                    ) => {
                        let binding = BoundRoute::new(
                            owner
                                .member(*module)
                                .ok_or_else(invalid)?
                                .bind_call_route(*target)?,
                        );
                        Ok(
                            if function
                                .metadata
                                .self_tail_calls
                                .binary_search(&call.pc)
                                .is_ok()
                            {
                                LinkedCall::SelfTail(Box::new(TailLink::new(
                                    binding,
                                    owner.key(),
                                    function.id,
                                )))
                            } else {
                                LinkedCall::Direct(binding)
                            },
                        )
                    }
                    (PreparedCallTarget::Closure(_), CallTarget::ClosureRegister { .. }) => {
                        Ok(LinkedCall::Physical)
                    }
                    _ => Err(invalid()),
                }
            })
            .collect()
    }
}
