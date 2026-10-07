//! Core scalar operations over closed engine representation facts.
use crate::language::semantics as traits;
use crate::types::TypeId;
use kagari_types::{
    language::Protocol,
    scalar::BuiltinType,
    surface::{BuiltinTypeFamily, builtin_type_spec},
};

/// Recognizes intrinsic signed/unsigned integer and floating-point families.
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

/// Checks intrinsic signed integer/float negation; unsigned types are excluded.
pub fn supports_unary_negation(ty: &TypeId) -> bool {
    matches!(
        builtin_family(ty),
        Some(BuiltinTypeFamily::SignedInteger | BuiltinTypeFamily::Float)
    )
}

/// Requires equal intrinsic numeric operand types.
pub fn supports_arithmetic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == rhs && is_numeric(lhs)
}

/// Checks the intrinsic same-numeric-type ordering rule.
pub fn supports_ordering(lhs: &TypeId, rhs: &TypeId) -> bool {
    supports_arithmetic(lhs, rhs)
}

/// Requires both operands to have intrinsic Bool type.
pub fn supports_boolean_logic(lhs: &TypeId, rhs: &TypeId) -> bool {
    lhs == &TypeId::Builtin(BuiltinType::Bool) && rhs == &TypeId::Builtin(BuiltinType::Bool)
}

/// Checks the intrinsic scalar specification's constant-safe type flag.
pub fn supports_const_type(ty: &TypeId) -> bool {
    match ty {
        TypeId::Builtin(builtin) => builtin_type_spec(*builtin).is_some_and(|spec| spec.const_safe),
        _ => false,
    }
}

/// Checks intrinsic Eq and Hash capabilities without a user implementation catalog.
pub fn supports_hash_key(ty: &TypeId) -> bool {
    traits::intrinsic_holds(Protocol::Eq, ty, None, &Default::default())
        && traits::intrinsic_holds(Protocol::Hash, ty, None, &Default::default())
}

fn builtin_family(ty: &TypeId) -> Option<BuiltinTypeFamily> {
    let TypeId::Builtin(builtin) = ty else {
        return None;
    };
    builtin_type_spec(*builtin).map(|spec| spec.family)
}
