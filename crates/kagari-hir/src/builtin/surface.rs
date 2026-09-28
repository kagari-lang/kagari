//! Source type operations over the engine-owned standard declaration surface.
use super::traits;
use crate::types::TypeId;
use kagari_abi::{
    scalar::BuiltinType,
    standard::{
        surface::{
            BuiltinTypeFamily, StandardTypeConstructor, builtin_type_spec, range_kind,
            standard_enum, standard_type_constructor,
        },
        traits::StandardTrait,
    },
};
use kagari_common::collection::CollectionAccess;

pub fn standard_enum_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_enum(name)?;
    (args.len() == spec.arity).then_some(TypeId::StandardEnum {
        kind: spec.kind,
        args,
    })
}

pub fn standard_generic_type(name: &str, args: Vec<TypeId>) -> Option<TypeId> {
    let spec = standard_type_constructor(name)?;
    if args.len() != spec.arity {
        return None;
    }

    if let Some(kind) = range_kind(name) {
        return Some(TypeId::Range(
            Box::new(
                args.into_iter()
                    .next()
                    .unwrap_or(TypeId::Builtin(BuiltinType::Unit)),
            ),
            kind,
        ));
    }
    match spec.kind {
        StandardTypeConstructor::Range
        | StandardTypeConstructor::RangeInclusive
        | StandardTypeConstructor::RangeFrom
        | StandardTypeConstructor::RangeTo
        | StandardTypeConstructor::RangeToInclusive
        | StandardTypeConstructor::RangeFull => unreachable!(),
        StandardTypeConstructor::Bound
        | StandardTypeConstructor::Option
        | StandardTypeConstructor::Result => standard_enum_type(name, args),
        StandardTypeConstructor::LinkedHashMap => {
            let [key, value] = args.try_into().ok()?;
            Some(TypeId::Map {
                key: Box::new(key),
                value: Box::new(value),
                access: CollectionAccess::Mutable,
            })
        }
        StandardTypeConstructor::Iter => {
            let [item] = args.try_into().ok()?;
            Some(TypeId::Iter(Box::new(item)))
        }
        StandardTypeConstructor::LinkedHashSet | StandardTypeConstructor::ArrayList => {
            let [item] = args.try_into().ok()?;
            Some(if spec.kind == StandardTypeConstructor::ArrayList {
                TypeId::Array(Box::new(item), CollectionAccess::Mutable)
            } else {
                TypeId::Set(Box::new(item), CollectionAccess::Mutable)
            })
        }
    }
}

pub fn is_numeric(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(
            BuiltinTypeFamily::SignedInteger
                | BuiltinTypeFamily::UnsignedInteger
                | BuiltinTypeFamily::Float
        )
    )
}

pub fn supports_unary_negation(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(BuiltinTypeFamily::SignedInteger | BuiltinTypeFamily::Float)
    )
}

pub fn supports_arithmetic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == rhs && is_numeric(lhs)
}

pub fn supports_ordering(lhs: &TypeId, rhs: &TypeId) -> bool {
    supports_arithmetic(lhs, rhs)
}

pub fn supports_boolean_logic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == &TypeId::Builtin(BuiltinType::Bool) && rhs == &TypeId::Builtin(BuiltinType::Bool)
}

pub fn supports_const_type(ty: &TypeId) -> bool {
    match ty {
        TypeId::Builtin(builtin) => builtin_type_spec(*builtin).is_some_and(|spec| spec.const_safe),
        _ => false,
    }
}

pub fn supports_hash_key(ty: &TypeId) -> bool {
    traits::intrinsic_holds(StandardTrait::Eq, ty, None, &Default::default())
        && traits::intrinsic_holds(StandardTrait::Hash, ty, None, &Default::default())
}

fn builtin_family(ty: &TypeId) -> Option<BuiltinTypeFamily> {
    let TypeId::Builtin(builtin) = ty else {
        return None;
    };
    builtin_type_spec(*builtin).map(|spec| spec.family)
}
