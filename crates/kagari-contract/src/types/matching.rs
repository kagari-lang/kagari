//! Match checked implementation templates against a requested executable contract.
use crate::{
    language::Protocol,
    types::{
        AssociatedTypeFamily, GenericParam, InterfaceTable, NominalTy, Ty,
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};

use kagari_common::{cancellation::CancellationToken, identity::DefinitionPath};

/// The actual checked header, independently of its declaration/executable source.
#[derive(Clone, Copy)]
pub struct ImplementationPattern<'a> {
    pub parameters: &'a [GenericParam],
    pub receiver: &'a Ty,
    pub interface: &'a NominalTy,
}

pub fn match_implementation<'a>(
    table: &'a InterfaceTable,
    interface: &'a NominalTy,
    receiver: &'a Ty,
    cancel: &CancellationToken,
) -> Result<Option<TypeSubstitution<'a>>, TypeTransformError> {
    let Ty::Trait(implemented) = &table.trait_type else {
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
    // Readonly native capabilities admit either storage view. Other impls must
    // match access exactly; storage arguments remain invariant in either case.
    let readonly = Protocol::from_id(&implemented.declaration).is_some_and(|kind| {
        matches!(
            kind,
            Protocol::List | Protocol::Map | Protocol::Set | Protocol::Iterable | Protocol::Index
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
            (Ty::Parameter { owner, position }, actual)
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
                Ty::StandardEnum {
                    kind: a,
                    args: left,
                },
                Ty::StandardEnum {
                    kind: b,
                    args: right,
                },
            ) if a == b && left.len() == right.len() => pending.extend(left.iter().zip(right)),
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
            (Ty::Array(left, _), Ty::Array(right, _)) | (Ty::Set(left, _), Ty::Set(right, _)) => {
                pending.push((left, right))
            }
            (
                Ty::Map {
                    key: lk, value: lv, ..
                },
                Ty::Map {
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
    families: &[AssociatedTypeFamily],
    interface: &NominalTy,
    receiver: &Ty,
    member: &DefinitionPath,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        scalar::BuiltinType,
        types::{AssociatedTypeFamily, GenericParam},
    };
    use kagari_common::{
        collection::CollectionAccess,
        identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id},
    };
    use std::slice;

    fn id(kind: DefinitionKind, name: &str) -> DefinitionPath {
        DefinitionPath {
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
        let integer = Ty::Builtin(BuiltinType::I32);
        let mutable = Ty::Array(Box::new(integer.clone()), CollectionAccess::Mutable);
        let readonly = Ty::Array(Box::new(integer.clone()), CollectionAccess::ReadOnly);
        let parameter = GenericParam {
            owner: id(DefinitionKind::Impl, ""),
            position: 0,
        };
        let mut interface =
            crate::language::primitive::applied(Protocol::List, vec![integer.clone()]);
        let mut table = InterfaceTable {
            declaration: parameter.owner.clone(),
            name: "List".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
            trait_type: Ty::Trait(crate::language::primitive::applied(
                Protocol::List,
                vec![parameter.as_type()],
            )),
            associated_type_families: vec![],
            host_bridge: false,
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
        interface.declaration = crate::language::identity(Protocol::MutableList);
        let Ty::Trait(implemented) = &mut table.trait_type else {
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
        interface = crate::language::primitive::applied(Protocol::List, vec![readonly.clone()]);
        table.trait_type = Ty::Trait(crate::language::primitive::applied(
            Protocol::List,
            vec![parameter.as_type()],
        ));
        table.for_type = Ty::Array(Box::new(mutable), CollectionAccess::Mutable);
        let nested = Ty::Array(Box::new(readonly), CollectionAccess::ReadOnly);
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
        let parameter = GenericParam {
            owner: implementation.clone(),
            position: 0,
        };
        let family_parameter = GenericParam {
            owner: associated_type_id(&implementation, "Item"),
            position: 0,
        };
        let member = associated_type_id(&interface_id, "Item");
        let table = InterfaceTable {
            declaration: implementation,
            name: "Read".into(),
            generic_params: vec![parameter.clone()],
            bounds: vec![],
            methods: vec![],
            associated_consts: vec![],
            for_type: Ty::Tuple(vec![parameter.as_type(), parameter.as_type()]),
            trait_type: Ty::Trait(NominalTy {
                declaration: interface_id.clone(),
                arguments: vec![parameter.as_type()],
                associated_types: Default::default(),
            }),
            associated_type_families: vec![AssociatedTypeFamily {
                declaration: member.clone(),
                generic_params: vec![family_parameter.clone()],
                bounds: vec![],
                value: Ty::Tuple(vec![parameter.as_type(), family_parameter.as_type()]),
            }],
            host_bridge: false,
        };
        let integer = Ty::Builtin(BuiltinType::I32);
        let boolean = Ty::Builtin(BuiltinType::Bool);
        let interface = NominalTy {
            declaration: interface_id,
            arguments: vec![integer.clone()],
            associated_types: Default::default(),
        };
        let cancel = CancellationToken::default();
        let receiver = Ty::Tuple(vec![integer.clone(), integer.clone()]);
        assert!(
            match_implementation(&table, &interface, &receiver, &cancel)
                .unwrap()
                .is_some()
        );
        let mismatch = Ty::Tuple(vec![integer.clone(), boolean.clone()]);
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
                        Ty::Trait(value) => value,
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
            Some(Ty::Tuple(vec![integer, boolean]))
        );
        cancel.cancel();
        assert!(matches!(
            match_implementation(&table, &interface, &receiver, &cancel),
            Err(TypeTransformError::Cancelled)
        ));
    }
}
