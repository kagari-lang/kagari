//! Checked operations supplied by a generic caller, independent of native binding.
use crate::{
    callable::{CallableImplementation, interface::InterfaceCallContract},
    declaration::ModuleDecl,
    effects::EffectSet,
    native_import::{
        NativeSignature,
        callables::{NativeCallableApplication, NativeCallableOrigin, NativeCallableRequirement},
    },
    types::{
        AbiType, ConcreteFunctionIdentity, ConstraintAbi, GenericBoundAbi, GenericParameterAbi,
        TraitAbi, inheritance,
        proofs::ProofCatalog,
        substitution::{TypeSubstitution, TypeTransformError},
        verify::{concrete_type_valid, types_in_scope},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath},
};
use serde::{Deserialize, Serialize};

/// A concrete selection is checked against the supplying program. Forwarding
/// retains the caller's existing selection and its generation; it never resolves
/// a trait again at execution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationWitness {
    Selected(Box<NativeCallableApplication>),
    SharedMethod(Box<SharedMethodWitness>),
    Forward(Box<NativeCallableRequirement>),
}

/// Select the implementation table once; method-local arguments are supplied by
/// each checked call into that table's shared entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedMethodWitness {
    pub requirement: NativeCallableRequirement,
    pub implementation: ConcreteFunctionIdentity,
}

impl OperationWitness {
    pub fn structurally_valid(&self, parameters: &[GenericParameterAbi]) -> bool {
        match self {
            Self::SharedMethod(selected) => {
                selected.implementation.declaration.within_path_limit()
                    && selected.implementation.arguments.len() <= 4096
                    && selected.requirement.member.within_path_limit()
                    && selected.requirement.arguments.is_empty()
                    && types_in_scope(
                        selected.implementation.arguments.iter().chain([
                            &selected.requirement.receiver,
                            &AbiType::Trait(selected.requirement.interface.clone()),
                        ]),
                        parameters,
                        &Default::default(),
                    )
            }
            Self::Forward(required) => {
                !parameters.is_empty()
                    && required.member.within_path_limit()
                    && required.arguments.len() <= 4096
                    && types_in_scope(
                        required.arguments.iter().chain([
                            &required.receiver,
                            &AbiType::Trait(required.interface.clone()),
                        ]),
                        parameters,
                        &Default::default(),
                    )
            }
            Self::Selected(call) => {
                let valid = |ty: &AbiType| {
                    ty.within_wire_limits()
                        && concrete_type_valid(ty, &CancellationToken::default())
                };
                call.effects == EffectSet::native_call()
                    && (call.origin != NativeCallableOrigin::ProtocolAdapter
                        || call.implementation == CallableImplementation::Script)
                    && call.instance.declaration.within_path_limit()
                    && call.instance.arguments.len() <= 4096
                    && call.instance.arguments.iter().all(valid)
                    && call.requirement.member.within_path_limit()
                    && call.requirement.arguments.len() <= 4096
                    && call.requirement.arguments.iter().all(valid)
                    && valid(&call.requirement.receiver)
                    && valid(&AbiType::Trait(call.requirement.interface.clone()))
                    && call.signature.params.len() <= 4096
                    && call.signature.params.iter().all(valid)
                    && valid(&call.signature.result)
                    && match &call.implementation {
                        CallableImplementation::Script => true,
                        CallableImplementation::Native(binding) => binding.within_path_limit(),
                        CallableImplementation::Required
                        | CallableImplementation::NativeDefault(_) => false,
                    }
            }
        }
    }

    pub fn requirement(&self) -> &NativeCallableRequirement {
        match self {
            Self::Selected(selected) => &selected.requirement,
            Self::SharedMethod(selected) => &selected.requirement,
            Self::Forward(required) => required,
        }
    }

    pub fn valid(
        &self,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParameterAbi],
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if !self.structurally_valid(parameters) {
            return Ok(false);
        }
        match self {
            Self::SharedMethod(selected) => {
                catalog.shared_method_matches(selected, parameters, assumptions, cancel)
            }
            Self::Selected(selected) => {
                catalog.callable_matches(&selected.requirement, selected, cancel)
            }
            Self::Forward(required) => Ok(required_operations(
                assumptions,
                &|id| catalog.trait_contract(id),
                cancel,
            )?
            .iter()
            .any(|available| {
                available.receiver == required.receiver
                    && available.interface == required.interface
                    && available.member == required.member
            }) && (required.is_generic_member(catalog)
                || required
                    .signature_in_scope(catalog, parameters, assumptions, cancel)?
                    .is_some())),
        }
    }
}

/// Enumerate the member operations guaranteed by these bounds, including
/// inherited traits. This is declaration work, never executable-body inspection.
pub fn required_operations<'a>(
    bounds: &[GenericBoundAbi],
    lookup: &impl Fn(&DefinitionPath) -> Option<&'a TraitAbi>,
    cancel: &CancellationToken,
) -> Result<Vec<NativeCallableRequirement>, TypeTransformError> {
    let mut operations = vec![];
    for bound in bounds {
        for constraint in &bound.constraints {
            let ConstraintAbi::Trait(interface) = constraint else {
                continue;
            };
            for interface in inheritance::trait_closure(interface, &bound.ty, cancel, lookup)? {
                let contract =
                    lookup(&interface.declaration).ok_or(TypeTransformError::InvalidContract)?;
                for method in &contract.methods {
                    cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                    if method.params.first().is_none_or(|param| {
                        param.ty != AbiType::SelfType(interface.declaration.clone())
                    }) {
                        continue;
                    }
                    let operation = NativeCallableRequirement {
                        receiver: bound.ty.clone(),
                        interface: interface.clone(),
                        member: ModuleDecl::method_id(&interface.declaration, &method.name),
                        arguments: vec![],
                    };
                    if !operations.contains(&operation) {
                        operations.push(operation);
                    }
                    if operations.len() > 4096 {
                        return Err(TypeTransformError::LimitExceeded);
                    }
                }
            }
        }
    }
    Ok(operations)
}

impl NativeCallableRequirement {
    pub fn is_generic_member(&self, catalog: &ProofCatalog<'_>) -> bool {
        self.arguments.is_empty()
            && catalog
                .trait_contract(&self.interface.declaration)
                .is_some_and(|contract| {
                    contract.methods.iter().any(|method| {
                        !method.generic_params.is_empty()
                            && self.member
                                == ModuleDecl::method_id(&self.interface.declaration, &method.name)
                    })
                })
    }

    /// Instantiate a member declaration with a raw constrained receiver, rather
    /// than the boxed interface receiver used by ordinary interface dispatch.
    pub fn signature(
        &self,
        contract: &TraitAbi,
        cancel: &CancellationToken,
    ) -> Result<NativeSignature, TypeTransformError> {
        let slot = contract
            .methods
            .iter()
            .position(|method| {
                let mut member = self.interface.declaration.clone();
                let Some(segment) = self.member.path.last() else {
                    return false;
                };
                member.path.push(segment.clone());
                member == self.member
                    && segment.name == method.name
                    && segment.kind == DefinitionKind::Method
                    && segment.occurrence == 0
            })
            .ok_or(TypeTransformError::InvalidContract)?;
        let call = InterfaceCallContract {
            receiver: Some(self.receiver.clone()),
            operations: vec![],
            interface: self.interface.clone(),
            method_slot: slot as u32,
            arguments: self.arguments.clone(),
        };
        let signature = call.signature(contract, cancel)?;
        Ok(NativeSignature {
            params: signature.params,
            result: signature.result,
        })
    }

    pub fn signature_in_scope(
        &self,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParameterAbi],
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<Option<NativeSignature>, TypeTransformError> {
        let Some(contract) = catalog.trait_contract(&self.interface.declaration) else {
            return Ok(None);
        };
        let signature = self.signature(contract, cancel)?;
        if !types_in_scope(
            signature
                .params
                .iter()
                .chain([&signature.result])
                .chain(&self.arguments),
            parameters,
            cancel,
        ) || !catalog.holds(&self.interface, &self.receiver, assumptions, cancel)?
        {
            return Ok(None);
        }
        let method = contract
            .methods
            .iter()
            .find(|method| {
                self.member
                    .path
                    .last()
                    .is_some_and(|member| member.name == method.name)
            })
            .ok_or(TypeTransformError::InvalidContract)?;
        let mut substitution = TypeSubstitution::default();
        substitution.bind_receiver(&self.interface.declaration, &self.receiver);
        for (parameter, argument) in contract
            .generic_params
            .iter()
            .zip(&self.interface.arguments)
            .chain(method.generic_params.iter().zip(&self.arguments))
        {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        for bound in substitution.apply_bounds(&method.bounds, cancel)? {
            if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(None);
            }
        }
        Ok(Some(signature))
    }
}
