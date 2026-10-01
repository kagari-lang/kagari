//! Match checked implementation templates against a requested executable contract.
use crate::{
    standard::traits::StandardTrait,
    types::{
        AbiType, AssociatedTypeFamilyAbi, GenericParameterAbi, InterfaceTableAbi, NominalAbiType,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};

use kagari_common::{cancellation::CancellationToken, identity::DefinitionId};

/// The actual checked header, independently of its declaration/executable source.
#[derive(Clone, Copy)]
pub struct ImplementationPattern<'a> {
    pub parameters: &'a [GenericParameterAbi],
    pub receiver: &'a AbiType,
    pub interface: &'a NominalAbiType,
}

pub fn match_implementation<'a>(
    table: &'a InterfaceTableAbi,
    interface: &'a NominalAbiType,
    receiver: &'a AbiType,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let AbiType::Trait(implemented) = &table.trait_type else {
        return Err(TypeTransformError::InvalidContract);
    };
    match_pattern(
        ImplementationPattern {
            parameters: &table.generic_params,
            receiver: &table.for_type,
            interface: implemented,
        },
        interface,
        receiver,
        cancel,
    )
}

pub fn match_pattern<'a>(
    pattern: ImplementationPattern<'a>,
    interface: &'a NominalAbiType,
    receiver: &'a AbiType,
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
    // Readonly native capabilities admit either storage view. Other impls must
    // match access exactly; storage arguments remain invariant in either case.
    let readonly = StandardTrait::from_id(&implemented.declaration).is_some_and(|kind| {
        matches!(
            kind,
            StandardTrait::List
                | StandardTrait::Map
                | StandardTrait::Set
                | StandardTrait::Iterable
                | StandardTrait::Index
        )
    });
    let mut bindings = TypeSubstitution::default();
    let mut pending = vec![(pattern.receiver, receiver)];
    pending.extend(implemented.arguments.iter().zip(&interface.arguments));
    let mut remaining = MAX_TYPE_NODES * 2;
    while let Some((template, actual)) = pending.pop() {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        if remaining == 0 {
            return Err(TypeTransformError::LimitExceeded);
        }
        remaining -= 1;
        match (template, actual) {
            (AbiType::Parameter { owner, position }, actual)
                if pattern
                    .parameters
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
            (AbiType::Iter(left), AbiType::Iter(right)) => pending.push((left, right)),
            (AbiType::Array(left, _), AbiType::Array(right, _))
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
    let target = bindings.apply(pattern.receiver, cancel)?;
    if target != *receiver && !(readonly && target.can_weaken_to(receiver)) {
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

pub(crate) fn projection_output(
    pattern: ImplementationPattern<'_>,
    families: &[AssociatedTypeFamilyAbi],
    interface: &NominalAbiType,
    receiver: &AbiType,
    member: &DefinitionId,
    arguments: &[AbiType],
    cancel: &CancellationToken,
) -> Result<Option<AbiType>, TypeTransformError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scalar::BuiltinType,
        types::{AssociatedTypeFamilyAbi, GenericParameterAbi},
    };
    use kagari_common::{
        collection::CollectionAccess,
        identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id},
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
    fn native_templates_weaken_only_readonly_outer_access() {
        let integer = AbiType::Builtin(BuiltinType::I32);
        let mutable = AbiType::Array(Box::new(integer.clone()), CollectionAccess::Mutable);
        let readonly = AbiType::Array(Box::new(integer.clone()), CollectionAccess::ReadOnly);
        let parameter = GenericParameterAbi {
            owner: id(DefinitionKind::Impl, ""),
            position: 0,
        };
        let mut interface =
            crate::standard::intrinsic::applied(StandardTrait::List, vec![integer.clone()]);
        let mut table = InterfaceTableAbi {
            declaration: parameter.owner.clone(),
            name: "List".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
            trait_type: AbiType::Trait(crate::standard::intrinsic::applied(
                StandardTrait::List,
                vec![parameter.as_type()],
            )),
            associated_type_families: vec![],
            host_bridge: false,
            native_bridge: false,
        };
        let cancel = CancellationToken::default();
        assert!(
            match_implementation(&table, &interface, &mutable, &cancel)
                .unwrap()
                .is_some()
        );
        assert!(
            match_implementation(&table, &interface, &readonly, &cancel)
                .unwrap()
                .is_some()
        );
        interface.declaration = crate::standard::traits::identity(StandardTrait::MutableList);
        let AbiType::Trait(implemented) = &mut table.trait_type else {
            unreachable!()
        };
        implemented.declaration = interface.declaration.clone();
        assert!(
            match_implementation(&table, &interface, &mutable, &cancel)
                .unwrap()
                .is_some()
        );
        assert!(
            match_implementation(&table, &interface, &readonly, &cancel)
                .unwrap()
                .is_none()
        );
        // Generic arguments cannot acquire the outer access relaxation.
        interface =
            crate::standard::intrinsic::applied(StandardTrait::List, vec![readonly.clone()]);
        table.trait_type = AbiType::Trait(crate::standard::intrinsic::applied(
            StandardTrait::List,
            vec![parameter.as_type()],
        ));
        table.for_type = AbiType::Array(Box::new(mutable), CollectionAccess::Mutable);
        let nested = AbiType::Array(Box::new(readonly), CollectionAccess::ReadOnly);
        assert!(
            match_implementation(&table, &interface, &nested, &cancel)
                .unwrap()
                .is_none()
        );
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
                ImplementationPattern {
                    parameters: &table.generic_params,
                    receiver: &table.for_type,
                    interface: match &table.trait_type {
                        AbiType::Trait(value) => value,
                        _ => unreachable!(),
                    },
                },
                &table.associated_type_families,
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
