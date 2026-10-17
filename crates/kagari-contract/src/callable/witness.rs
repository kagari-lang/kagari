//! Checked operations supplied by a generic caller, independent of native binding.
use {
    crate::{
        callable::{adapter::SharedAdapterWitness, interface::InterfaceCallContract},
        effects::EffectSet,
        native_import::callables::{NativeCallableApplication, NativeCallableOrigin},
        types::{ConcreteFunctionIdentity, proofs::ProofCatalog},
    },
    kagari_types::{
        callable::{CallableImplementation, Signature},
        declaration::{TraitDef, requirement::NativeCallableRequirement, verify::types_in_scope},
        ty::{
            GenericBound, GenericParam, Ty, inheritance,
            substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
        },
    },
};

use std::iter;

use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};

/// A concrete selection is checked against the supplying program. Forwarding
/// retains the caller's existing selection and its generation; it never resolves
/// a trait again at execution time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OperationWitness {
    Selected(Box<NativeCallableApplication>),
    SharedMethod(Box<SharedMethodWitness>),
    SharedAdapter(Box<SharedAdapterWitness>),
    Forward(Box<NativeCallableRequirement>),
}

/// Select the implementation table once, retaining caller-scoped receiver arguments.
/// Receiver-bound operations remain attached to this selection: for example,
/// selecting `Vec<T>.eq` under `T: PartialEq` retains the supplied `T.eq` operation.
/// Method-local arguments are supplied by each checked call into the shared entry.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SharedMethodWitness {
    pub requirement: NativeCallableRequirement,
    pub implementation: ConcreteFunctionIdentity,
    /// Operations promised by the selected implementation's receiver bounds.
    pub operations: Vec<OperationWitness>,
}

impl OperationWitness {
    /// Direct component or receiver-bound operations, in their checked order.
    pub fn children(&self) -> &[Self] {
        match self {
            Self::SharedAdapter(adapter) => &adapter.call.operations,
            Self::SharedMethod(method) => &method.operations,
            Self::Selected(_) | Self::Forward(_) => &[],
        }
    }

    /// Walk composed witnesses without recursive metadata traversal.
    pub fn walk(&self) -> impl Iterator<Item = &Self> {
        let mut pending = vec![self];
        iter::from_fn(move || {
            let operation = pending.pop()?;
            pending.extend(operation.children().iter().rev());
            Some(operation)
        })
    }

    fn within_limits(&self) -> bool {
        let mut pending = vec![(self, 0)];
        let mut count = 0;
        while let Some((operation, depth)) = pending.pop() {
            count += 1;
            if count > 4096 || depth > 64 {
                return false;
            }
            if operation.children().len() > 4096 {
                return false;
            }
            pending.extend(operation.children().iter().map(|child| (child, depth + 1)));
        }
        true
    }

    pub fn structurally_valid(
        &self,
        parameters: &[GenericParam],
        definitions: &DefinitionTable,
    ) -> bool {
        if !self.within_limits() {
            return false;
        }
        match self {
            Self::SharedAdapter(adapter) => {
                adapter.template(definitions).is_some()
                    && adapter
                        .call
                        .structurally_valid(parameters, &Default::default(), definitions)
            }
            Self::SharedMethod(selected) => {
                selected
                    .operations
                    .iter()
                    .all(|operation| operation.structurally_valid(parameters, definitions))
                    && definitions
                        .resolve(selected.implementation.declaration)
                        .is_ok()
                    && selected.implementation.arguments.len() <= 4096
                    && definitions.resolve(selected.requirement.member).is_ok()
                    && selected.requirement.arguments.is_empty()
                    && types_in_scope(
                        selected.implementation.arguments.iter().chain([
                            &selected.requirement.receiver,
                            &Ty::Trait(selected.requirement.interface.clone()),
                        ]),
                        parameters,
                        &Default::default(),
                        definitions,
                    )
            }
            Self::Forward(required) => {
                !parameters.is_empty()
                    && definitions.resolve(required.member).is_ok()
                    && required.arguments.len() <= 4096
                    && types_in_scope(
                        required
                            .arguments
                            .iter()
                            .chain([&required.receiver, &Ty::Trait(required.interface.clone())]),
                        parameters,
                        &Default::default(),
                        definitions,
                    )
            }
            Self::Selected(call) => {
                let valid = |ty: &Ty| {
                    ty.within_wire_limits()
                        && types_in_scope([ty], &[], &CancellationToken::default(), definitions)
                };
                call.effects == EffectSet::native_call()
                    && (call.origin != NativeCallableOrigin::ProtocolAdapter
                        || call.implementation == CallableImplementation::Script)
                    && definitions.resolve(call.instance.declaration).is_ok()
                    && call.instance.arguments.len() <= 4096
                    && call.instance.arguments.iter().all(valid)
                    && definitions.resolve(call.requirement.member).is_ok()
                    && call.requirement.arguments.len() <= 4096
                    && call.requirement.arguments.iter().all(valid)
                    && valid(&call.requirement.receiver)
                    && valid(&Ty::Trait(call.requirement.interface.clone()))
                    && call.signature.params.len() <= 4096
                    && call.signature.params.iter().all(valid)
                    && valid(&call.signature.result)
                    && match &call.implementation {
                        CallableImplementation::Script => true,
                        CallableImplementation::Native(binding) => {
                            definitions.resolve(*binding).is_ok()
                        }
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
        let definitions = catalog.definitions();
        if !self.structurally_valid(parameters, definitions) {
            return Ok(false);
        }
        match self {
            Self::SharedAdapter(adapter) => adapter.valid(catalog, parameters, assumptions, cancel),
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
                definitions,
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

impl OperationWitness {
    pub fn requirement(&self) -> &NativeCallableRequirement {
        match self {
            Self::SharedAdapter(adapter) => &adapter.requirement,
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
    lookup: &impl Fn(&DefinitionId) -> Option<&'a TraitDef>,
    cancel: &CancellationToken,
    definitions: &DefinitionTable,
) -> Result<Vec<NativeCallableRequirement>, TypeTransformError> {
    // A bound also promises its immediate associated-type contracts. In
    // particular Iterable supplies the selected next for its Iter projection.
    let mut expanded = bounds.to_vec();
    for bound in bounds {
        for interface in &bound.constraints {
            for interface in inheritance::trait_closure(interface, &bound.ty, cancel, lookup)? {
                let contract =
                    lookup(&interface.declaration).ok_or(TypeTransformError::InvalidContract)?;
                let mut substitution = TypeSubstitution::default();
                substitution.bind_receiver(&interface.declaration, &bound.ty);
                for (parameter, argument) in
                    contract.generic_params.iter().zip(&interface.arguments)
                {
                    substitution.bind(&parameter.owner, parameter.position, argument);
                }
                for member in &contract.associated_types {
                    if !member.generic_params.is_empty() || member.bounds.is_empty() {
                        continue;
                    }
                    let apply = |ty: &Ty| {
                        resolve_associated_outputs(
                            &substitution.apply(ty, cancel)?,
                            &interface,
                            &bound.ty,
                            cancel,
                        )
                    };
                    let receiver = apply(&Ty::Projection {
                        receiver: Box::new(Ty::SelfType(interface.declaration)),
                        interface: Box::new(interface.clone()),
                        member: member.declaration,
                        arguments: vec![],
                    })?;
                    let constraints = member
                        .bounds
                        .iter()
                        .map(|required| {
                            let Ty::Trait(required) = apply(&Ty::Trait(required.clone()))? else {
                                return Err(TypeTransformError::InvalidContract);
                            };
                            Ok(required)
                        })
                        .collect::<Result<_, _>>()?;
                    expanded.push(GenericBound {
                        ty: receiver,
                        constraints,
                    });
                    if expanded.len() > 4096 {
                        return Err(TypeTransformError::LimitExceeded);
                    }
                }
            }
        }
    }
    let mut operations = vec![];
    for bound in &expanded {
        for interface in &bound.constraints {
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
                        member: definitions
                            .lookup_child(
                                interface.declaration,
                                DefinitionKind::Method,
                                &method.name,
                                0,
                            )
                            .ok_or(TypeTransformError::InvalidContract)?,
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
                        && Some(requirement.member)
                            == catalog.definitions().lookup_child(
                                requirement.interface.declaration,
                                DefinitionKind::Method,
                                &method.name,
                                0,
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
    definitions: &DefinitionTable,
) -> Result<Signature, TypeTransformError> {
    let slot = contract
        .methods
        .iter()
        .position(|method| {
            definitions.parent(requirement.member) == Ok(Some(requirement.interface.declaration))
                && definitions
                    .resolve(requirement.member)
                    .ok()
                    .and_then(|view| view.last())
                    .is_some_and(|segment| {
                        segment.name == method.name
                            && segment.kind == DefinitionKind::Method
                            && segment.occurrence == 0
                    })
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
    let signature = call.signature(contract, cancel, definitions)?;
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
    let definitions = catalog.definitions();
    let Some(contract) = catalog.trait_contract(&requirement.interface.declaration) else {
        return Ok(None);
    };
    let signature = requirement_signature(requirement, contract, cancel, definitions)?;
    if !types_in_scope(
        signature
            .params
            .iter()
            .chain([&signature.result])
            .chain(&requirement.arguments),
        parameters,
        cancel,
        definitions,
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
            definitions
                .resolve(requirement.member)
                .ok()
                .and_then(|view| view.last())
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
    for bound in substitution.apply_bounds(&method.bounds, definitions, cancel)? {
        if !catalog.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
            return Ok(None);
        }
    }
    Ok(Some(signature))
}

mod mapping;
