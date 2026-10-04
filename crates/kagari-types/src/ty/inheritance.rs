//! Applied trait ancestry over portable declaration contracts.
use crate::{
    declaration::TraitDef,
    language::{Protocol, identity},
    ty::{
        Constraint, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError, resolve_associated_outputs},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, associated_type_id},
};
use std::collections::HashSet;

const MAX_TRAITS: usize = 4_096;
const MAX_PATH: usize = 64;
const MAX_EDGES: usize = 100_000;

/// The additional dynamic surface supported by the language's Iterable contract.
/// Explicit implementation outputs remain the canonical static signature.
pub fn erased_iterator_view<'a>(
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
    lookup: &impl Fn(&DefinitionPath) -> Option<&'a TraitDef>,
) -> Result<Option<NominalTy>, TypeTransformError> {
    if interface.declaration != identity(Protocol::Iterable) {
        return Ok(None);
    }
    let mut erased = interface.clone();
    erased
        .associated_types
        .remove(&associated_type_id(&interface.declaration, "Iter"));
    let view = interface_views(&erased, receiver, cancel, lookup)?.remove(0);
    Ok((view != *interface).then_some(view))
}

/// Concrete dynamic values hide Iterable's iterator behind its declared bound.
/// Implementation ancestry remains unchanged: a view is not a second impl.
pub fn interface_views<'a>(
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
    lookup: &impl Fn(&DefinitionPath) -> Option<&'a TraitDef>,
) -> Result<Vec<NominalTy>, TypeTransformError> {
    let mut closure = trait_closure(interface, receiver, cancel, lookup)?;
    for parent in &mut closure {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if parent.declaration != identity(Protocol::Iterable) {
            continue;
        }
        let iter = associated_type_id(&parent.declaration, "Iter");
        if parent.associated_types.contains_key(&iter) {
            continue;
        }
        let contract = lookup(&parent.declaration).ok_or(TypeTransformError::InvalidContract)?;
        let Some(output) = contract
            .associated_types
            .iter()
            .find(|output| output.declaration == iter)
        else {
            continue;
        };
        let [Constraint::Trait(bound)] = output.bounds.as_slice() else {
            continue;
        };
        if !output.generic_params.is_empty() || bound.declaration != identity(Protocol::Iterator) {
            continue;
        }
        let mut parameters = TypeSubstitution::default();
        for (parameter, argument) in contract.generic_params.iter().zip(&parent.arguments) {
            parameters.bind(&parameter.owner, parameter.position, argument);
        }
        let bound = Ty::Trait(parameters.apply_nominal(bound, cancel)?);
        let bound = resolve_associated_outputs(&bound, parent, cancel)?;
        // The caller validates the binder scope. Shared bodies preserve their
        // parameters here just as they do in the surrounding interface type.
        parent.associated_types.insert(iter, bound);
    }
    Ok(closure)
}

/// Preserve applied arguments and associated bindings while walking supertraits.
/// Declaration cycles fail even when they change arguments. Diamond paths to the
/// same application are deduplicated; distinct applications retain their identity.
pub fn trait_closure<'a>(
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
    lookup: &impl Fn(&DefinitionPath) -> Option<&'a TraitDef>,
) -> Result<Vec<NominalTy>, TypeTransformError> {
    let empty = TypeSubstitution::default();
    let root = empty.apply_nominal(interface, cancel)?;
    let receiver = empty.apply(receiver, cancel)?;
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![(root, Vec::<DefinitionPath>::new())];
    let mut remaining = MAX_EDGES;
    while let Some((applied, mut path)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if path.contains(&applied.declaration) || path.len() >= MAX_PATH || remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        if !seen.insert(applied.clone()) {
            continue;
        }
        if result.len() >= MAX_TRAITS {
            return Err(TypeTransformError::LimitExceeded);
        }
        let contract = lookup(&applied.declaration).ok_or(TypeTransformError::InvalidContract)?;
        if contract.generic_params.len() != applied.arguments.len() {
            return Err(TypeTransformError::InvalidContract);
        }
        if contract.supertraits.len() > remaining
            || pending.len() > remaining - contract.supertraits.len()
        {
            return Err(TypeTransformError::LimitExceeded);
        }
        let mut parameters = TypeSubstitution::default();
        for (parameter, argument) in contract.generic_params.iter().zip(&applied.arguments) {
            parameters.bind(&parameter.owner, parameter.position, argument);
        }
        let mut receivers = TypeSubstitution::default();
        receivers.bind_receiver(&applied.declaration, &receiver);
        path.push(applied.declaration.clone());
        for parent in contract.supertraits.iter().rev() {
            let parent = Ty::Trait(empty.apply_nominal(parent, cancel)?);
            let parent = resolve_associated_outputs(&parent, &applied, cancel)?;
            let parent = receivers.apply(&parent, cancel)?;
            let parent = parameters.apply(&parent, cancel)?;
            let Ty::Trait(parent) = parent else {
                return Err(TypeTransformError::InvalidContract);
            };
            pending.push((parent, path.clone()));
        }
        result.push(applied);
    }
    Ok(result)
}
