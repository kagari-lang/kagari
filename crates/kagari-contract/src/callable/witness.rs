//! Checked operations supplied by a generic caller, independent of native binding.
use {
    crate::{
        callable::interface::InterfaceCallContract,
        effects::EffectSet,
        native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
        types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    },
    kagari_types::{
        callable::{CallableImplementation, Signature},
        declaration::{
            TraitDef,
            module::ModuleDecl,
            requirement::NativeCallableRequirement,
            verify::{concrete_type_valid, types_in_scope},
        },
        ty::{
            Constraint, GenericBound, GenericParam, Ty, inheritance,
            substitution::{TypeSubstitution, TypeTransformError},
        },
    },
};

use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionKind, DefinitionPath, reference::DefinitionReference},
};
use serde::{Deserialize, Serialize};

/// A concrete selection is checked against the supplying program. Forwarding
/// retains the caller's existing selection and its generation; it never resolves
/// a trait again at execution time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub enum OperationWitness<I = DefinitionPath> {
    Selected(Box<NativeCallableApplication<I>>),
    SharedMethod(Box<SharedMethodWitness<I>>),
    Forward(Box<NativeCallableRequirement<I>>),
}

/// Select the implementation table once; method-local arguments are supplied by
/// each checked call into that table's shared entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "I: DefinitionReference + serde::Serialize",
    deserialize = "I: DefinitionReference + serde::Deserialize<'de>"
))]
pub struct SharedMethodWitness<I = DefinitionPath> {
    pub requirement: NativeCallableRequirement<I>,
    pub implementation: ConcreteFunctionIdentity<I>,
}

impl OperationWitness {
    pub fn structurally_valid(&self, parameters: &[GenericParam]) -> bool {
        match self {
            Self::SharedMethod(selected) => {
                selected.implementation.declaration.within_path_limit()
                    && selected.implementation.arguments.len() <= 4096
                    && selected.requirement.member.within_path_limit()
                    && selected.requirement.arguments.is_empty()
                    && types_in_scope(
                        selected.implementation.arguments.iter().chain([
                            &selected.requirement.receiver,
                            &Ty::Trait(selected.requirement.interface.clone()),
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
                        required
                            .arguments
                            .iter()
                            .chain([&required.receiver, &Ty::Trait(required.interface.clone())]),
                        parameters,
                        &Default::default(),
                    )
            }
            Self::Selected(call) => {
                let valid = |ty: &Ty| {
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
                    && valid(&Ty::Trait(call.requirement.interface.clone()))
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

    pub fn valid(
        &self,
        catalog: &ProofCatalog<'_>,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
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
            }) && (is_generic_member(required, catalog)
                || requirement_signature_in_scope(
                    required,
                    catalog,
                    parameters,
                    assumptions,
                    cancel,
                )?
                .is_some())),
        }
    }
}

impl<I: DefinitionReference> OperationWitness<I> {
    pub fn requirement(&self) -> &NativeCallableRequirement<I> {
        match self {
            Self::Selected(selected) => &selected.requirement,
            Self::SharedMethod(selected) => &selected.requirement,
            Self::Forward(required) => required,
        }
    }
}

/// Enumerate the member operations guaranteed by these bounds, including
/// inherited traits. This is declaration work, never executable-body inspection.
pub fn required_operations<'a>(
    bounds: &[GenericBound],
    lookup: &impl Fn(&DefinitionPath) -> Option<&'a TraitDef>,
    cancel: &CancellationToken,
) -> Result<Vec<NativeCallableRequirement>, TypeTransformError> {
    let mut operations = vec![];
    for bound in bounds {
        for constraint in &bound.constraints {
            let Constraint::Trait(interface) = constraint else {
                continue;
            };
            for interface in inheritance::trait_closure(interface, &bound.ty, cancel, lookup)? {
                let contract =
                    lookup(&interface.declaration).ok_or(TypeTransformError::InvalidContract)?;
                for method in &contract.methods {
                    cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                    // Static members are also promised by a bound (for example
                    // From<E>::from used by a native FromResidual implementation).
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

pub fn is_generic_member(
    requirement: &NativeCallableRequirement,
    catalog: &ProofCatalog<'_>,
) -> bool {
    requirement.arguments.is_empty()
        && catalog
            .trait_contract(&requirement.interface.declaration)
            .is_some_and(|contract| {
                contract.methods.iter().any(|method| {
                    !method.generic_params.is_empty()
                        && requirement.member
                            == ModuleDecl::method_id(
                                &requirement.interface.declaration,
                                &method.name,
                            )
                })
            })
}

/// Instantiate a member declaration with a raw constrained receiver, rather
/// than the boxed interface receiver used by ordinary interface dispatch.
pub fn requirement_signature(
    requirement: &NativeCallableRequirement,
    contract: &TraitDef,
    cancel: &CancellationToken,
) -> Result<Signature, TypeTransformError> {
    let slot = contract
        .methods
        .iter()
        .position(|method| {
            let mut member = requirement.interface.declaration.clone();
            let Some(segment) = requirement.member.path.last() else {
                return false;
            };
            member.path.push(segment.clone());
            member == requirement.member
                && segment.name == method.name
                && segment.kind == DefinitionKind::Method
                && segment.occurrence == 0
        })
        .ok_or(TypeTransformError::InvalidContract)?;
    let call = InterfaceCallContract {
        normalizations: vec![],
        receiver: Some(requirement.receiver.clone()),
        operations: vec![],
        interface: requirement.interface.clone(),
        method_slot: slot as u32,
        arguments: requirement.arguments.clone(),
    };
    let signature = call.signature(contract, cancel)?;
    Ok(Signature {
        params: signature.params,
        result: signature.result,
    })
}

pub fn requirement_signature_in_scope(
    requirement: &NativeCallableRequirement,
    catalog: &ProofCatalog<'_>,
    parameters: &[GenericParam],
    assumptions: &[GenericBound],
    cancel: &CancellationToken,
) -> Result<Option<Signature>, TypeTransformError> {
    let Some(contract) = catalog.trait_contract(&requirement.interface.declaration) else {
        return Ok(None);
    };
    let signature = requirement_signature(requirement, contract, cancel)?;
    if !types_in_scope(
        signature
            .params
            .iter()
            .chain([&signature.result])
            .chain(&requirement.arguments),
        parameters,
        cancel,
    ) || !catalog.holds(
        &requirement.interface,
        &requirement.receiver,
        assumptions,
        cancel,
    )? {
        return Ok(None);
    }
    let method = contract
        .methods
        .iter()
        .find(|method| {
            requirement
                .member
                .path
                .last()
                .is_some_and(|member| member.name == method.name)
        })
        .ok_or(TypeTransformError::InvalidContract)?;
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&requirement.interface.declaration, &requirement.receiver);
    for (parameter, argument) in contract
        .generic_params
        .iter()
        .zip(&requirement.interface.arguments)
        .chain(method.generic_params.iter().zip(&requirement.arguments))
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

mod mapping;
