//! Match checked implementation templates against a requested executable contract.
use crate::types::substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError};
use crate::types::{AbiType, InterfaceTableAbi, NominalAbiType};
use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};

pub fn match_implementation<'a>(
    table: &'a InterfaceTableAbi,
    interface: &'a NominalAbiType,
    receiver: &'a AbiType,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let AbiType::Trait(implemented) = &table.trait_type else {
        return Err(TypeTransformError::InvalidContract);
    };
    if table.generic_params.len() > MAX_TYPE_NODES
        || !table.for_type.within_wire_limits()
        || !table.trait_type.within_wire_limits()
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
    let mut bindings = TypeSubstitution::default();
    let mut pending = vec![(&table.for_type, receiver)];
    pending.extend(implemented.arguments.iter().zip(&interface.arguments));
    let mut remaining = MAX_TYPE_NODES * 2;
    while let Some((pattern, actual)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        match (pattern, actual) {
            (AbiType::Parameter { owner, position }, actual)
                if table
                    .generic_params
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
            (AbiType::Struct(left), AbiType::Struct(right))
            | (AbiType::Enum(left), AbiType::Enum(right))
            | (AbiType::Trait(left), AbiType::Trait(right)) => {
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
            (AbiType::Tuple(left), AbiType::Tuple(right)) if left.len() == right.len() => {
                pending.extend(left.iter().zip(right))
            }
            (
                AbiType::StandardEnum {
                    kind: a,
                    args: left,
                },
                AbiType::StandardEnum {
                    kind: b,
                    args: right,
                },
            ) if a == b && left.len() == right.len() => pending.extend(left.iter().zip(right)),
            (
                AbiType::Function {
                    params: left,
                    result: lr,
                },
                AbiType::Function {
                    params: right,
                    result: rr,
                },
            ) if left.len() == right.len() => {
                pending.push((lr, rr));
                pending.extend(left.iter().zip(right));
            }
            (AbiType::Range(left, a), AbiType::Range(right, b)) if a == b => {
                pending.push((left, right))
            }
            (AbiType::Iter(left), AbiType::Iter(right))
            | (AbiType::Array(left, _), AbiType::Array(right, _))
            | (AbiType::Set(left, _), AbiType::Set(right, _)) => pending.push((left, right)),
            (
                AbiType::Map {
                    key: lk, value: lv, ..
                },
                AbiType::Map {
                    key: rk, value: rv, ..
                },
            ) => pending.extend([(lk.as_ref(), rk.as_ref()), (lv.as_ref(), rv.as_ref())]),
            (left, right) if left == right => {}
            _ => return Ok(None),
        }
        if pending.len() > MAX_TYPE_NODES * 2 {
            return Err(TypeTransformError::LimitExceeded);
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

pub(crate) fn projection_output(
    table: &InterfaceTableAbi,
    interface: &NominalAbiType,
    receiver: &AbiType,
    member: &DefinitionId,
    arguments: &[AbiType],
    cancel: &CancellationToken,
) -> Result<Option<AbiType>, TypeTransformError> {
    let Some(mut substitution) = match_implementation(table, interface, receiver, cancel)? else {
        return Ok(None);
    };
    if !arguments.is_empty() {
        let Some(family) = table
            .associated_type_families
            .iter()
            .find(|family| family.declaration == *member)
        else {
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
    let AbiType::Trait(implemented) = &table.trait_type else {
        return Err(TypeTransformError::InvalidContract);
    };
    implemented
        .associated_types
        .get(member)
        .map(|value| substitution.apply(value, cancel))
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scalar::BuiltinType,
        types::{AssociatedTypeFamilyAbi, GenericParameterAbi},
    };
    use kagari_common::identity::{
        DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
    };
    use std::slice;

    fn id(kind: DefinitionKind, name: &str) -> DefinitionId {
        DefinitionId {
            module: ModuleIdentity::single_file("match.kgr"),
            path: vec![DefinitionPathSegment {
                kind,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn implementation_matching_keeps_repeated_binders_consistent_and_applies_families() {
        let implementation = id(DefinitionKind::Impl, "");
        let interface_id = id(DefinitionKind::Trait, "Read");
        let parameter = GenericParameterAbi {
            owner: implementation.clone(),
            position: 0,
        };
        let family_parameter = GenericParameterAbi {
            owner: associated_type_id(&implementation, "Item"),
            position: 0,
        };
        let member = associated_type_id(&interface_id, "Item");
        let table = InterfaceTableAbi {
            declaration: implementation,
            name: "Read".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: AbiType::Tuple(vec![parameter.as_type(), parameter.as_type()]),
            trait_type: AbiType::Trait(NominalAbiType {
                declaration: interface_id.clone(),
                arguments: vec![parameter.as_type()],
                associated_types: Default::default(),
            }),
            associated_type_families: vec![AssociatedTypeFamilyAbi {
                declaration: member.clone(),
                generic_params: vec![family_parameter.clone()],
                bounds: vec![],
                value: AbiType::Tuple(vec![parameter.as_type(), family_parameter.as_type()]),
            }],
            host_bridge: false,
            native_bridge: false,
        };
        let integer = AbiType::Builtin(BuiltinType::I32);
        let boolean = AbiType::Builtin(BuiltinType::Bool);
        let interface = NominalAbiType {
            declaration: interface_id,
            arguments: vec![integer.clone()],
            associated_types: Default::default(),
        };
        let cancel = CancellationToken::default();
        let receiver = AbiType::Tuple(vec![integer.clone(), integer.clone()]);
        assert!(
            match_implementation(&table, &interface, &receiver, &cancel)
                .unwrap()
                .is_some()
        );
        let mismatch = AbiType::Tuple(vec![integer.clone(), boolean.clone()]);
        assert!(
            match_implementation(&table, &interface, &mismatch, &cancel)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            projection_output(
                &table,
                &interface,
                &receiver,
                &member,
                slice::from_ref(&boolean),
                &cancel
            )
            .unwrap(),
            Some(AbiType::Tuple(vec![integer, boolean]))
        );
        cancel.cancel();
        assert!(matches!(
            match_implementation(&table, &interface, &receiver, &cancel),
            Err(TypeTransformError::Cancelled)
        ));
    }
}
