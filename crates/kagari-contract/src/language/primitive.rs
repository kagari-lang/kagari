//! Scalar operators, callable output and Iterator's identity Iterable rule.
//! Collections and library traits are proved from carried implementation records.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{
    conversion as numeric,
    declaration::TraitDef,
    language,
    language::Protocol,
    scalar::BuiltinType,
    ty::{
        GenericBound, NominalTy, Ty,
        substitution::{TypeSubstitution, TypeTransformError},
    },
};
use std::collections::BTreeMap;

pub fn applied(
    kind: Protocol,
    arguments: Vec<Ty>,
    definitions: &DefinitionTable,
) -> Result<NominalTy, TypeTransformError> {
    Ok(NominalTy {
        declaration: definitions
            .lookup(&language::identity(kind))
            .ok_or(TypeTransformError::InvalidContract)?,
        arguments,
        associated_types: BTreeMap::new(),
    })
}

pub fn requirements(
    interface: &NominalTy,
    receiver: &Ty,
    cancel: &CancellationToken,
    contract: Option<&TraitDef>,
    definitions: &DefinitionTable,
) -> Result<Option<Vec<GenericBound>>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    if let Some((interface, target)) = contract
        .and_then(|contract| contract.conversion_adapter.as_ref())
        .and_then(|adapter| adapter.reverse_requirement(interface, receiver))
    {
        return Ok(Some(vec![GenericBound {
            ty: target,
            constraints: vec![interface],
        }]));
    }
    if Protocol::from_definition(&interface.declaration, definitions) == Some(Protocol::From)
        && interface.associated_types.is_empty()
        && (interface.arguments.as_slice() == [receiver.clone()]
            || matches!((receiver, interface.arguments.as_slice()), (Ty::Builtin(target), [Ty::Builtin(source)]) if numeric::lossless_from(*source, *target)))
    {
        return Ok(Some(vec![]));
    }
    if let Some(output) = operator_output(interface, receiver, definitions) {
        return Ok(interface
            .associated_types
            .iter()
            .all(|(member, ty)| {
                Some(*member)
                    == definitions.lookup_child(
                        interface.declaration,
                        DefinitionKind::AssociatedType,
                        "Output",
                        0,
                    )
                    && *ty == output
            })
            .then(Vec::new));
    }
    Ok(
        identity_iterator(interface, receiver, definitions).map(|required| {
            vec![GenericBound {
                ty: receiver.clone(),
                constraints: vec![required],
            }]
        }),
    )
}

pub fn associated_output(
    interface: &NominalTy,
    receiver: &Ty,
    member: &DefinitionId,
    cancel: &CancellationToken,
    contract: Option<&TraitDef>,
    definitions: &DefinitionTable,
) -> Result<Option<Ty>, TypeTransformError> {
    let copier = TypeSubstitution::default();
    copier.apply_nominal(interface, cancel)?;
    copier.apply(receiver, cancel)?;
    if let Some(output) = contract
        .and_then(|contract| contract.conversion_adapter.as_ref())
        .and_then(|adapter| adapter.associated_output(interface, receiver, member))
    {
        return Ok(Some(output));
    }
    Ok(
        if Some(*member)
            == definitions.lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Output",
                0,
            )
        {
            operator_output(interface, receiver, definitions)
        } else {
            None
        },
    )
}

fn operator_output(
    interface: &NominalTy,
    receiver: &Ty,
    definitions: &DefinitionTable,
) -> Option<Ty> {
    let kind = Protocol::from_definition(&interface.declaration, definitions)?;
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
        && let Ty::Array(element) = receiver
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

pub fn identity_iterator(
    interface: &NominalTy,
    receiver: &Ty,
    definitions: &DefinitionTable,
) -> Option<NominalTy> {
    if Protocol::from_definition(&interface.declaration, definitions) != Some(Protocol::Iterable)
        || !interface.arguments.is_empty()
    {
        return None;
    }
    let mut required = applied(Protocol::Iterator, vec![], definitions).ok()?;
    for (member, ty) in &interface.associated_types {
        if Some(*member)
            == definitions.lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
        {
            required.associated_types.insert(
                definitions.lookup_child(
                    required.declaration,
                    DefinitionKind::AssociatedType,
                    "Item",
                    0,
                )?,
                ty.clone(),
            );
        } else if Some(*member)
            != definitions.lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Iter",
                0,
            )
            || ty != receiver
        {
            return None;
        }
    }
    Some(required)
}
