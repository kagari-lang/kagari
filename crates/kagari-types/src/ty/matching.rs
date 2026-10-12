//! Match checked implementation templates against a requested executable contract.
use crate::{
    collection::CollectionAccess,
    declaration::AssociatedTypeFamily,
    language::Protocol,
    ty::{
        GenericParam, NominalTy, Ty,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};

use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionId};

/// The actual checked header, independently of its declaration/executable source.
pub struct ImplementationPattern<'a> {
    pub parameters: &'a [GenericParam],
    pub receiver: &'a Ty,
    pub interface: &'a NominalTy,
    /// Language role resolved by the owner of the interface identity.
    pub protocol: Option<Protocol>,
    pub storage_access: Option<CollectionAccess>,
}

impl Copy for ImplementationPattern<'_> {}

impl Clone for ImplementationPattern<'_> {
    fn clone(&self) -> Self {
        *self
    }
}

pub fn match_pattern<'a>(
    pattern: ImplementationPattern<'a>,
    interface: &'a NominalTy,
    receiver: &'a Ty,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let implemented = pattern.interface;
    if pattern.parameters.len() > MAX_TYPE_NODES
        || !pattern.receiver.within_wire_limits()
        || !receiver.within_wire_limits()
    {
        return Err(TypeTransformError::LimitExceeded);
    }
    if implemented.declaration != interface.declaration
        || implemented.arguments.len() != interface.arguments.len()
    {
        return Ok(None);
    }
    // Validate the requested nominal as one tree before comparisons or borrowing
    // nested arguments into the returned substitution.
    TypeSubstitution::default().apply_nominal(interface, cancel)?;
    TypeSubstitution::default().apply_nominal(implemented, cancel)?;
    let mut pending = vec![(pattern.receiver, receiver)];
    pending.extend(implemented.arguments.iter().zip(&interface.arguments));
    let Some(bindings) = match_pairs(pattern.parameters, pending, cancel)? else {
        return Ok(None);
    };
    let target = bindings.apply(pattern.receiver, cancel)?;
    // Implementation identity includes the receiver's access descriptor. Host
    // adaptation is checked at its boundary, never inferred from a trait marker.
    if target != *receiver {
        return Ok(None);
    }
    for (template, actual) in implemented.arguments.iter().zip(&interface.arguments) {
        if bindings.apply(template, cancel)? != *actual {
            return Ok(None);
        }
    }
    for (member, expected) in &interface.associated_types {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let Some(actual) = implemented.associated_types.get(member) else {
            return Ok(None);
        };
        if bindings.apply(actual, cancel)? != *expected {
            return Ok(None);
        }
    }
    Ok(Some(bindings))
}

/// Match an inherent receiver without weakening access or resolving new proofs.
pub fn match_receiver<'a>(
    parameters: &'a [GenericParam],
    template: &'a Ty,
    actual: &'a Ty,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    if parameters.len() > MAX_TYPE_NODES
        || !template.within_wire_limits()
        || !actual.within_wire_limits()
    {
        return Err(TypeTransformError::LimitExceeded);
    }
    let Some(bindings) = match_pairs(parameters, vec![(template, actual)], cancel)? else {
        return Ok(None);
    };
    if bindings.apply(template, cancel)? != *actual {
        return Ok(None);
    }
    Ok(Some(bindings))
}

fn match_pairs<'a>(
    parameters: &'a [GenericParam],
    mut pending: Vec<(&'a Ty, &'a Ty)>,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let mut bindings = TypeSubstitution::default();
    let mut remaining = MAX_TYPE_NODES * 2;
    while let Some((template, actual)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        match (template, actual) {
            (Ty::Parameter { owner, position }, actual)
                if parameters
                    .iter()
                    .any(|p| p.owner == *owner && p.position == *position) =>
            {
                if let Some(bound) = bindings.parameter(owner, *position) {
                    if bound != actual {
                        return Ok(None);
                    }
                } else {
                    bindings.bind(owner, *position, actual);
                }
            }
            (Ty::Struct(left), Ty::Struct(right))
            | (Ty::NativeObject(left), Ty::NativeObject(right))
            | (Ty::Enum(left), Ty::Enum(right))
            | (Ty::Trait(left), Ty::Trait(right)) => {
                if left.declaration != right.declaration
                    || left.arguments.len() != right.arguments.len()
                    || !left
                        .associated_types
                        .keys()
                        .eq(right.associated_types.keys())
                {
                    return Ok(None);
                }
                pending.extend(left.arguments.iter().zip(&right.arguments));
                pending.extend(
                    left.associated_types
                        .values()
                        .zip(right.associated_types.values()),
                );
            }
            (Ty::Tuple(left), Ty::Tuple(right)) if left.len() == right.len() => {
                pending.extend(left.iter().zip(right))
            }

            (
                Ty::Function {
                    params: left,
                    result: lr,
                },
                Ty::Function {
                    params: right,
                    result: rr,
                },
            ) if left.len() == right.len() => {
                pending.push((lr, rr));
                pending.extend(left.iter().zip(right));
            }
            (Ty::Range(left, a), Ty::Range(right, b)) if a == b => pending.push((left, right)),
            (Ty::Iter(left), Ty::Iter(right)) => pending.push((left, right)),
            (Ty::Array(left), Ty::Array(right)) => pending.push((left, right)),
            (Ty::Set(left, a), Ty::Set(right, b)) if a == b => pending.push((left, right)),
            (
                Ty::Map {
                    key: lk,
                    value: lv,
                    access: la,
                },
                Ty::Map {
                    key: rk,
                    value: rv,
                    access: ra,
                },
            ) if la == ra => {
                pending.extend([(lk.as_ref(), rk.as_ref()), (lv.as_ref(), rv.as_ref())])
            }
            (left, right) if left == right => {}
            _ => return Ok(None),
        }
        if pending.len() > MAX_TYPE_NODES * 2 {
            return Err(TypeTransformError::LimitExceeded);
        }
    }
    Ok(Some(bindings))
}

pub fn projection_output(
    pattern: ImplementationPattern<'_>,
    families: &[AssociatedTypeFamily],
    interface: &NominalTy,
    receiver: &Ty,
    member: &DefinitionId,
    arguments: &[Ty],
    cancel: &CancellationToken,
) -> Result<Option<Ty>, TypeTransformError> {
    let Some(mut substitution) = match_pattern(pattern, interface, receiver, cancel)? else {
        return Ok(None);
    };
    if !arguments.is_empty() {
        let Some(family) = families.iter().find(|family| family.declaration == *member) else {
            return Ok(None);
        };
        if family.generic_params.len() != arguments.len() {
            return Err(TypeTransformError::InvalidContract);
        }
        for (parameter, argument) in family.generic_params.iter().zip(arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        return substitution.apply(&family.value, cancel).map(Some);
    }
    pattern
        .interface
        .associated_types
        .get(member)
        .map(|value| substitution.apply(value, cancel))
        .transpose()
}
