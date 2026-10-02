//! Scalar operators, callable output and Iterator's identity Iterable rule.
//! Collections and library traits are proved from carried implementation records.
use crate::{
    language::{self, Protocol},
    numeric,
    scalar::BuiltinType,
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionId, associated_type_id},
};
use std::collections::BTreeMap;

pub fn applied(kind: Protocol, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: language::identity(kind),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

pub fn requirements(
    interface: &NominalAbiType,
    receiver: &AbiType,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBoundAbi>>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    if Protocol::from_id(&interface.declaration) == Some(Protocol::From)
        && interface.associated_types.is_empty()
        && (interface.arguments.as_slice() == [receiver.clone()]
            || matches!((receiver, interface.arguments.as_slice()), (AbiType::Builtin(target), [AbiType::Builtin(source)]) if numeric::lossless_from(*source, *target)))
    {
        return Ok(Some(vec![]));
    }
    if let Some(output) = operator_output(interface, receiver) {
        return Ok(interface
            .associated_types
            .iter()
            .all(|(member, ty)| {
                *member == associated_type_id(&interface.declaration, "Output") && *ty == output
            })
            .then(Vec::new));
    }
    Ok(identity_iterator(interface, receiver).map(|required| {
        vec![GenericBoundAbi {
            ty: receiver.clone(),
            constraints: vec![ConstraintAbi::Trait(required)],
        }]
    }))
}

pub fn associated_output(
    interface: &NominalAbiType,
    receiver: &AbiType,
    member: &DefinitionId,
    cancel: &CancellationToken,
) -> Result<Option<AbiType>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    Ok(
        if *member == associated_type_id(&interface.declaration, "Output") {
            operator_output(interface, receiver)
        } else {
            None
        },
    )
}

fn operator_output(interface: &NominalAbiType, receiver: &AbiType) -> Option<AbiType> {
    let kind = Protocol::from_id(&interface.declaration)?;
    if kind == Protocol::Fn
        && let AbiType::Function { params, result } = receiver
    {
        let arguments = if params.is_empty() {
            AbiType::Builtin(BuiltinType::Unit)
        } else {
            AbiType::Tuple(params.clone())
        };
        return (interface.arguments == [arguments]).then(|| result.as_ref().clone());
    }
    if kind == Protocol::Index
        && let AbiType::Array(element, _) = receiver
    {
        return matches!(interface.arguments.as_slice(), [AbiType::Builtin(ty)] if ty.integer_layout().is_some()).then(|| element.as_ref().clone());
    }
    let AbiType::Builtin(lhs) = receiver else {
        return None;
    };
    if interface.arguments.is_empty() {
        let unary = match kind {
            Protocol::Not => *lhs == BuiltinType::Bool || lhs.integer_layout().is_some(),
            Protocol::Neg => {
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
        Protocol::Shl | Protocol::Shr => {
            lhs.integer_layout().is_some() && rhs.integer_layout().is_some()
        }
        Protocol::BitAnd | Protocol::BitOr | Protocol::BitXor => {
            lhs == rhs && lhs.integer_layout().is_some()
        }
        _ => kind.binary_operator() && lhs == rhs && lhs.number_type().is_some(),
    };
    binary.then(|| receiver.clone())
}

pub fn identity_iterator(interface: &NominalAbiType, receiver: &AbiType) -> Option<NominalAbiType> {
    if Protocol::from_id(&interface.declaration) != Some(Protocol::Iterable)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let mut required = applied(Protocol::Iterator, vec![]);
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
