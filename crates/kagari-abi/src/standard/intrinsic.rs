//! Portable intrinsic trait contracts and their remaining proof obligations.
//! Nominal overrides, generic assumptions and recursive structural protocols are
//! resolved by the linked catalog, using the obligations returned here.
use crate::numeric;
use crate::scalar::BuiltinType;
use crate::standard::implementation;
use crate::standard::surface::STANDARD_IMPLEMENTATIONS;
use crate::standard::traits::{self, StandardTrait};
use crate::types::substitution::{TypeSubstitution, TypeTransformError};
use crate::types::{AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::{DefinitionId, associated_type_id};
use std::collections::BTreeMap;

pub fn applied(kind: StandardTrait, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: traits::identity(kind),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

/// None means no intrinsic rule applies. An empty list is an unconditional
/// intrinsic proof; a nonempty list must be discharged under the caller's budget.
pub fn requirements(
    interface: &NominalAbiType,
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return Ok(None);
    };
    if kind.reverse_conversion() {
        return Ok(
            reverse_conversion(interface, receiver).map(|(required, target)| {
                vec![GenericBoundAbi {
                    ty: target,
                    constraints: vec![ConstraintAbi::Trait(required)],
                }]
            }),
        );
    }
    let output = if kind.operator() {
        operator_output(interface, receiver).map(|output| ("Output", output))
    } else if kind == StandardTrait::FromStr && interface.arguments.is_empty() {
        parsing_error(receiver).map(|error| ("Err", error))
    } else if kind == StandardTrait::TryFrom {
        conversion_error(interface, receiver).map(|error| ("Error", error))
    } else {
        None
    };
    if let Some((name, output)) = output {
        return Ok(interface
            .associated_types
            .iter()
            .all(|(member, ty)| {
                *member == associated_type_id(&interface.declaration, name) && *ty == output
            })
            .then(Vec::new));
    }
    if kind == StandardTrait::From && interface.associated_types.is_empty() {
        let identity = interface.arguments.as_slice() == [receiver.clone()];
        let numeric = match (receiver, interface.arguments.as_slice()) {
            (AbiType::Builtin(target), [AbiType::Builtin(source)]) => {
                numeric::lossless_from(*source, *target)
            }
            _ => false,
        };
        return Ok((identity || numeric).then(Vec::new));
    }
    if kind.aggregation() {
        return Ok((interface.associated_types.is_empty()
            && interface.arguments.as_slice() == [receiver.clone()]
            && matches!(receiver, AbiType::Builtin(ty) if ty.number_type().is_some()))
        .then(Vec::new));
    }
    for declaration in STANDARD_IMPLEMENTATIONS {
        if declaration.interface != kind.name() {
            continue;
        }
        if let Some(bindings) =
            implementation::match_application(declaration, interface, receiver, cancel)?
        {
            return implementation::requirements(declaration, &bindings, cancel).map(Some);
        }
    }
    // Iterator supplies identity Iterable; the linked solver proves Iterator,
    // including any requested Item output, rather than assuming it is available.
    Ok(identity_iterator(interface, receiver).map(|required| {
        vec![GenericBoundAbi {
            ty: receiver.clone(),
            constraints: vec![ConstraintAbi::Trait(required)],
        }]
    }))
}

/// Outputs supplied directly by scalar operators, conversions and generated
/// declarations. The caller separately proves the complete applied contract.
pub fn associated_output(
    interface: &NominalAbiType,
    receiver: &AbiType,
    member: &DefinitionId,
    cancel: &CancellationToken,
) -> Result<Option<AbiType>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
        return Ok(None);
    };
    if *member == associated_type_id(&interface.declaration, "Output") && kind.operator() {
        return Ok(operator_output(interface, receiver));
    }
    if *member == associated_type_id(&interface.declaration, "Err")
        && kind == StandardTrait::FromStr
        && interface.arguments.is_empty()
    {
        return Ok(parsing_error(receiver));
    }
    if *member == associated_type_id(&interface.declaration, "Error") {
        if kind == StandardTrait::TryFrom {
            return Ok(conversion_error(interface, receiver));
        }
        if let Some((required, target)) = reverse_conversion(interface, receiver) {
            return Ok(Some(AbiType::Projection {
                receiver: Box::new(target),
                member: associated_type_id(&required.declaration, "Error"),
                interface: Box::new(required),
                arguments: vec![],
            }));
        }
    }
    let declared_kind = if kind == StandardTrait::Iterable && matches!(receiver, AbiType::Iter(_)) {
        StandardTrait::Iterator
    } else {
        kind
    };
    for declaration in STANDARD_IMPLEMENTATIONS {
        if declaration.interface != declared_kind.name() {
            continue;
        }
        let requested = applied(declared_kind, interface.arguments.clone());
        if let Some(bindings) =
            implementation::match_application(declaration, &requested, receiver, cancel)?
        {
            let contract = implementation::applied_contract(declaration, &bindings, cancel)?;
            if declared_kind == kind {
                return Ok(contract.associated_types.get(member).cloned());
            }
            if *member == associated_type_id(&interface.declaration, "Iter") {
                return Ok(Some(receiver.clone()));
            }
            if *member == associated_type_id(&interface.declaration, "Item") {
                return Ok(contract
                    .associated_types
                    .get(&associated_type_id(&contract.declaration, "Item"))
                    .cloned());
            }
        }
    }
    Ok(None)
}

fn parsing_error(receiver: &AbiType) -> Option<AbiType> {
    let AbiType::Builtin(kind) = receiver else {
        return None;
    };
    Some(AbiType::StandardEnum {
        kind: numeric::parsing_error(*kind)?,
        args: vec![],
    })
}
fn conversion_error(interface: &NominalAbiType, receiver: &AbiType) -> Option<AbiType> {
    let (AbiType::Builtin(target), [AbiType::Builtin(source)]) =
        (receiver, interface.arguments.as_slice())
    else {
        return None;
    };
    Some(AbiType::StandardEnum {
        kind: numeric::conversion_error(*source, *target)?,
        args: vec![],
    })
}

fn operator_output(interface: &NominalAbiType, receiver: &AbiType) -> Option<AbiType> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if kind == StandardTrait::Fn
        && let AbiType::Function { params, result } = receiver
    {
        let arguments = if params.is_empty() {
            AbiType::Builtin(BuiltinType::Unit)
        } else {
            AbiType::Tuple(params.clone())
        };
        return (interface.arguments == [arguments]).then(|| result.as_ref().clone());
    }
    if kind == StandardTrait::Index
        && let AbiType::Array(element, _) = receiver
    {
        return matches!(interface.arguments.as_slice(), [AbiType::Builtin(ty)] if ty.integer_layout().is_some()).then(|| element.as_ref().clone());
    }
    let AbiType::Builtin(lhs) = receiver else {
        return None;
    };
    if interface.arguments.is_empty() {
        let unary = match kind {
            StandardTrait::Not => *lhs == BuiltinType::Bool || lhs.integer_layout().is_some(),
            StandardTrait::Neg => {
                lhs.integer_layout().is_some_and(|(_, signed)| signed)
                    || matches!(lhs, BuiltinType::F32 | BuiltinType::F64)
            }
            _ => false,
        };
        return unary.then(|| receiver.clone());
    }
    let [AbiType::Builtin(rhs)] = interface.arguments.as_slice() else {
        return None;
    };
    let binary = match kind {
        StandardTrait::Shl | StandardTrait::Shr => {
            lhs.integer_layout().is_some() && rhs.integer_layout().is_some()
        }
        StandardTrait::BitAnd | StandardTrait::BitOr | StandardTrait::BitXor => {
            lhs == rhs && lhs.integer_layout().is_some()
        }
        _ => kind.binary_operator() && lhs == rhs && lhs.number_type().is_some(),
    };
    binary.then(|| receiver.clone())
}

fn reverse_conversion(
    interface: &NominalAbiType,
    receiver: &AbiType,
) -> Option<(NominalAbiType, AbiType)> {
    let kind = StandardTrait::from_id(&interface.declaration)?;
    if !kind.reverse_conversion() || interface.arguments.len() != 1 {
        return None;
    }
    let forward = if kind == StandardTrait::Into {
        StandardTrait::From
    } else {
        StandardTrait::TryFrom
    };
    let mut required = applied(forward, vec![receiver.clone()]);
    for (member, ty) in &interface.associated_types {
        if *member != associated_type_id(&interface.declaration, "Error")
            || !kind.fallible_conversion()
        {
            return None;
        }
        required.associated_types.insert(
            associated_type_id(&required.declaration, "Error"),
            ty.clone(),
        );
    }
    Some((required, interface.arguments[0].clone()))
}

pub fn identity_iterator(interface: &NominalAbiType, receiver: &AbiType) -> Option<NominalAbiType> {
    if StandardTrait::from_id(&interface.declaration) != Some(StandardTrait::Iterable)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let mut required = applied(StandardTrait::Iterator, vec![]);
    for (member, ty) in &interface.associated_types {
        if *member == associated_type_id(&interface.declaration, "Item") {
            required.associated_types.insert(
                associated_type_id(&required.declaration, "Item"),
                ty.clone(),
            );
        } else if *member != associated_type_id(&interface.declaration, "Iter") || ty != receiver {
            return None;
        }
    }
    Some(required)
}

#[cfg(test)]
mod tests;
