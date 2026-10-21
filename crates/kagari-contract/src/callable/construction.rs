//! Closed interface constructions reachable through shared executable bodies.
pub mod evidence;
pub mod requirements;
#[doc(hidden)]
pub mod wire;

use crate::{
    callable::{
        construction::evidence::{ConstructionSelection, ConstructionWitness},
        evidence::EvidenceRef,
        interface::InterfaceCallContract,
        witness::OperationWitness,
    },
    native_import::NativeImport,
    types::ConcreteFunctionIdentity,
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::ty::{
    GenericParam, NominalTy, Ty,
    substitution::{TypeSubstitution, TypeTransformError},
};
use std::collections::{HashSet, VecDeque};

#[cfg(test)]
mod tests;

/// The construction-relevant part of an executable body. For `box<T>(xs)` this
/// retains T's binder and the interface application whose arguments contain T.
pub struct ConstructionBody {
    pub identity: ConcreteFunctionIdentity,
    pub parameters: Vec<GenericParam>,
    pub steps: Vec<ConstructionStep>,
}

pub enum ConstructionStep {
    Interface(ConcreteFunctionIdentity),
    /// Capabilities explicitly selected at a call boundary, including empty traits.
    Evidence {
        operations: Vec<OperationWitness>,
        constructions: Vec<ConstructionWitness>,
    },
    Call {
        body: ConcreteFunctionIdentity,
        arguments: Vec<Ty>,
    },
    Dispatch(InterfaceCallContract),
}

/// A retained concrete interface slot and the shared body it can enter.
pub struct ConstructionMethod {
    pub interface: NominalTy,
    pub receiver: Ty,
    pub slot: u32,
    pub body: ConcreteFunctionIdentity,
    pub parameters: Vec<GenericParam>,
    /// Checked slot argument expressions, after applying the receiver binders.
    pub arguments: Vec<Ty>,
}

/// Extract an installed native entry's declared result conversion.
pub fn native_construction(import: &NativeImport) -> Option<ConstructionBody> {
    if import.result_adapter.is_none() && import.callables.is_empty() {
        return None;
    }
    let mut steps = vec![ConstructionStep::Evidence {
        operations: import.callables.clone(),
        constructions: vec![],
    }];
    if let Some(adapter) = &import.result_adapter {
        steps.push(ConstructionStep::Interface(adapter.implementation.clone()));
    }
    Some(ConstructionBody {
        identity: import.instance.clone(),
        parameters: import
            .generic
            .as_ref()
            .map_or_else(Vec::new, |body| body.parameters.clone()),
        steps,
    })
}

/// Enumerate checked interface applications without specializing shared code.
/// Both source emission and source-free validation can use this graph. It follows
/// exact retained slots, substitutes one binder layer per call and bounds cycles
/// by distinct closed body applications. It never searches trait implementations.
pub fn closed_constructions(
    bodies: &[ConstructionBody],
    methods: &[ConstructionMethod],
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<ConcreteFunctionIdentity>, TypeTransformError> {
    let mut pending: VecDeque<_> = bodies
        .iter()
        .enumerate()
        .filter(|(_, body)| body.parameters.is_empty())
        .map(|(index, _)| (index, Vec::new()))
        .collect();
    let mut seen: HashSet<_> = pending.iter().cloned().collect();
    let mut results = Vec::new();
    let mut instances = HashSet::new();
    while let Some((index, arguments)) = pending.pop_front() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if seen.len() > limit {
            return Err(TypeTransformError::LimitExceeded);
        }
        let body = &bodies[index];
        if body.parameters.len() != arguments.len() {
            return Err(TypeTransformError::InvalidContract);
        }
        let mut bindings = TypeSubstitution::default();
        for (parameter, argument) in body.parameters.iter().zip(&arguments) {
            bindings.bind(&parameter.owner, parameter.position, argument);
        }
        for step in &body.steps {
            cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
            match step {
                ConstructionStep::Evidence {
                    operations,
                    constructions,
                } => {
                    for instance in
                        selected_constructions(operations, constructions, &bindings, limit, cancel)?
                    {
                        if instance.arguments.iter().all(Ty::is_concrete)
                            && instances.insert(instance.clone())
                        {
                            results.push(instance);
                            if results.len() > limit {
                                return Err(TypeTransformError::LimitExceeded);
                            }
                        }
                    }
                }
                ConstructionStep::Interface(instance) => {
                    let instance = ConcreteFunctionIdentity {
                        declaration: instance.declaration,
                        arguments: apply(&bindings, &instance.arguments, cancel)?,
                    };
                    if instance.arguments.iter().all(Ty::is_concrete)
                        && instances.insert(instance.clone())
                    {
                        results.push(instance);
                        if results.len() > limit {
                            return Err(TypeTransformError::LimitExceeded);
                        }
                    }
                }
                ConstructionStep::Call { body, arguments } => {
                    let arguments = apply(&bindings, arguments, cancel)?;
                    enqueue(bodies, body, arguments, &mut pending, &mut seen, limit)?;
                }
                ConstructionStep::Dispatch(call) => {
                    let interface = bindings.apply_nominal(&call.interface, cancel)?;
                    let receiver = call
                        .receiver
                        .as_ref()
                        .map(|ty| bindings.apply(ty, cancel))
                        .transpose()?;
                    let arguments = apply(&bindings, &call.arguments, cancel)?;
                    for method in methods {
                        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
                        if method.slot != call.method_slot
                            || method.interface.declaration != interface.declaration
                            || method.interface.arguments != interface.arguments
                            || !interface.associated_types.iter().all(|(id, ty)| {
                                method.interface.associated_types.get(id) == Some(ty)
                            })
                            || receiver
                                .as_ref()
                                .is_some_and(|receiver| *receiver != method.receiver)
                        {
                            continue;
                        }
                        if method.parameters.len() != arguments.len() {
                            return Err(TypeTransformError::InvalidContract);
                        }
                        let mut method_bindings = TypeSubstitution::default();
                        for (parameter, argument) in method.parameters.iter().zip(&arguments) {
                            method_bindings.bind(&parameter.owner, parameter.position, argument);
                        }
                        let entry = apply(&method_bindings, &method.arguments, cancel)?;
                        enqueue(bodies, &method.body, entry, &mut pending, &mut seen, limit)?;
                    }
                }
            }
        }
    }
    Ok(results)
}

fn apply(
    bindings: &TypeSubstitution<'_>,
    arguments: &[Ty],
    cancel: &CancellationToken,
) -> Result<Vec<Ty>, TypeTransformError> {
    arguments
        .iter()
        .map(|ty| bindings.apply(ty, cancel))
        .collect()
}

fn enqueue(
    bodies: &[ConstructionBody],
    identity: &ConcreteFunctionIdentity,
    arguments: Vec<Ty>,
    pending: &mut VecDeque<(usize, Vec<Ty>)>,
    seen: &mut HashSet<(usize, Vec<Ty>)>,
    limit: usize,
) -> Result<(), TypeTransformError> {
    if arguments.iter().all(Ty::is_concrete)
        && let Some(index) = bodies.iter().position(|body| body.identity == *identity)
    {
        let application = (index, arguments);
        if seen.insert(application.clone()) {
            if seen.len() > limit {
                return Err(TypeTransformError::LimitExceeded);
            }
            pending.push_back(application);
        }
    }
    Ok(())
}

fn selected_constructions(
    operations: &[OperationWitness],
    constructions: &[ConstructionWitness],
    bindings: &TypeSubstitution<'_>,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<ConcreteFunctionIdentity>, TypeTransformError> {
    let mut pending: Vec<_> = operations
        .iter()
        .map(EvidenceRef::Operation)
        .chain(constructions.iter().map(EvidenceRef::Construction))
        .map(|node| (node, 0))
        .collect();
    let mut results = Vec::new();
    let mut count = 0;
    while let Some((node, depth)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        count += 1;
        if count > limit || depth > 64 {
            return Err(TypeTransformError::LimitExceeded);
        }
        let selected = match node {
            EvidenceRef::Construction(ConstructionWitness {
                selection: ConstructionSelection::Declared { implementation, .. },
                ..
            }) => Some(implementation),
            EvidenceRef::Operation(OperationWitness::SharedMethod(selected)) => {
                Some(&selected.implementation)
            }
            _ => None,
        };
        if let Some(selected) = selected {
            results.push(ConcreteFunctionIdentity {
                declaration: selected.declaration,
                arguments: apply(bindings, &selected.arguments, cancel)?,
            });
        }
        let (operations, constructions) = node.children();
        pending.extend(
            operations
                .iter()
                .map(|child| (EvidenceRef::Operation(child), depth + 1)),
        );
        pending.extend(
            constructions
                .iter()
                .map(|child| (EvidenceRef::Construction(child), depth + 1)),
        );
    }
    Ok(results)
}
