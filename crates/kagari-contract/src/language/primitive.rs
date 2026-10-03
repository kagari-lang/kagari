//! Scalar operators, callable output and Iterator's identity Iterable rule.
//! Collections and library traits are proved from carried implementation records.
use crate::{
    language::{self, Protocol},
    numeric,
    scalar::BuiltinType,
    types::{
        Constraint, GenericBound, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{DefinitionPath, associated_type_id},
};
use std::collections::BTreeMap;

pub fn applied(kind: Protocol, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: language::identity(kind),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

pub fn requirements(
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
) -> Result<Option<Vec<GenericBound>>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    if let Some((interface, target)) = conversion_requirement(interface, receiver) {
        return Ok(Some(vec![GenericBound {
            ty: target,
            constraints: vec![Constraint::Trait(interface)],
        }]));
    }
    if Protocol::from_id(&interface.declaration) == Some(Protocol::From)
        && interface.associated_types.is_empty()
        && (interface.arguments.as_slice() == [receiver.clone()]
            || matches!((receiver, interface.arguments.as_slice()), (Ty::Builtin(target), [Ty::Builtin(source)]) if numeric::lossless_from(*source, *target)))
    {
        return Ok(Some(vec![]));
    }
    if let Some(error) = conversion_error(interface, receiver) {
        return Ok(interface
            .associated_types
            .iter()
            .all(|(member, ty)| {
                *member == associated_type_id(&interface.declaration, "Error") && *ty == error
            })
            .then(Vec::new));
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
        vec![GenericBound {
            ty: receiver.clone(),
            constraints: vec![Constraint::Trait(required)],
        }]
    }))
}

pub fn associated_output(
    interface: &NominalTy,
    receiver: &Ty,
    member: &DefinitionPath,
    cancel: &CancellationToken,
) -> Result<Option<Ty>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    if *member == associated_type_id(&interface.declaration, "Error")
        && let Some(error) = conversion_error(interface, receiver)
    {
        return Ok(Some(error));
    }
    if !matches!(receiver, Ty::SelfType(_))
        && *member == associated_type_id(&interface.declaration, "Error")
        && let Some((required, target)) = conversion_requirement(interface, receiver)
    {
        return Ok(Some(Ty::Projection {
            receiver: Box::new(target),
            member: associated_type_id(&required.declaration, "Error"),
            interface: Box::new(required),
            arguments: vec![],
        }));
    }
    Ok(
        if *member == associated_type_id(&interface.declaration, "Output") {
            operator_output(interface, receiver)
        } else {
            None
        },
    )
}

fn operator_output(interface: &NominalTy, receiver: &Ty) -> Option<Ty> {
    let kind = Protocol::from_id(&interface.declaration)?;
    if kind == Protocol::Fn
        && let Ty::Function { params, result } = receiver
    {
        let arguments = if params.is_empty() {
            Ty::Builtin(BuiltinType::Unit)
        } else {
            Ty::Tuple(params.clone())
        };
        return (interface.arguments == [arguments]).then(|| result.as_ref().clone());
    }
    if kind == Protocol::Index
        && let Ty::Array(element, _) = receiver
    {
        return matches!(interface.arguments.as_slice(), [Ty::Builtin(ty)] if ty.integer_layout().is_some()).then(|| element.as_ref().clone());
    }
    let Ty::Builtin(lhs) = receiver else {
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
    let [Ty::Builtin(rhs)] = interface.arguments.as_slice() else {
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

pub fn identity_iterator(interface: &NominalTy, receiver: &Ty) -> Option<NominalTy> {
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

/// Into and TryInto reuse the destination conversion, including its error output.
pub fn conversion_requirement(interface: &NominalTy, receiver: &Ty) -> Option<(NominalTy, Ty)> {
    let kind = Protocol::from_id(&interface.declaration)?;
    let origin = kind.conversion_origin()?;
    let [target] = interface.arguments.as_slice() else {
        return None;
    };
    let mut required = applied(origin, vec![receiver.clone()]);
    for (member, ty) in &interface.associated_types {
        if kind != Protocol::TryInto
            || *member != associated_type_id(&interface.declaration, "Error")
        {
            return None;
        }
        required.associated_types.insert(
            associated_type_id(&required.declaration, "Error"),
            ty.clone(),
        );
    }
    Some((required, target.clone()))
}

fn conversion_error(interface: &NominalTy, receiver: &Ty) -> Option<Ty> {
    if Protocol::from_id(&interface.declaration) != Some(Protocol::TryFrom) {
        return None;
    }
    let (Ty::Builtin(target), [Ty::Builtin(source)]) = (receiver, interface.arguments.as_slice())
    else {
        return None;
    };
    Some(Ty::StandardEnum {
        kind: numeric::conversion_error(*source, *target)?,
        args: vec![],
    })
}
