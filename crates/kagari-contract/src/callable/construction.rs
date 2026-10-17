//! Closed interface constructions reachable through shared executable bodies.
use crate::{
    callable::interface::InterfaceCallContract, native_import::NativeImport,
    types::ConcreteFunctionIdentity,
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::ty::{
    GenericParam, NominalTy, Ty,
    substitution::{TypeSubstitution, TypeTransformError},
};
use std::collections::{HashSet, VecDeque};

/// The construction-relevant part of an executable body. For `box<T>(xs)` this
/// retains T's binder and the interface application whose arguments contain T.
pub struct ConstructionBody {
    pub identity: ConcreteFunctionIdentity,
    pub parameters: Vec<GenericParam>,
    pub steps: Vec<ConstructionStep>,
}

pub enum ConstructionStep {
    Interface(ConcreteFunctionIdentity),
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
    let adapter = import.result_adapter.as_ref()?;
    Some(ConstructionBody {
        identity: import.instance.clone(),
        parameters: import
            .generic
            .as_ref()
            .map_or_else(Vec::new, |body| body.parameters.clone()),
        steps: vec![ConstructionStep::Interface(adapter.implementation.clone())],
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
