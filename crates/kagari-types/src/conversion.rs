//! Shared scalar conversion admissibility and declared error types.
use crate::scalar::BuiltinType;
pub fn lossless_from(source: BuiltinType, target: BuiltinType) -> bool {
    if source == target {
        return source.number_type().is_some() || source == BuiltinType::Bool;
    }
    if source == BuiltinType::Bool {
        return target.integer_layout().is_some();
    }
    match target {
        BuiltinType::F32 => matches!(
            source,
            BuiltinType::I8 | BuiltinType::I16 | BuiltinType::U8 | BuiltinType::U16
        ),
        BuiltinType::F64 => matches!(
            source,
            BuiltinType::I8
                | BuiltinType::I16
                | BuiltinType::I32
                | BuiltinType::U8
                | BuiltinType::U16
                | BuiltinType::U32
                | BuiltinType::F32
        ),
        BuiltinType::ISize => {
            matches!(source, BuiltinType::I8 | BuiltinType::I16 | BuiltinType::U8)
        }
        BuiltinType::USize => matches!(source, BuiltinType::U8 | BuiltinType::U16),
        _ if matches!(source, BuiltinType::ISize | BuiltinType::USize) => false,
        _ => match (source.integer_layout(), target.integer_layout()) {
            (Some((a, sa)), Some((b, sb))) => (sa == sb && a < b) || (!sa && sb && a < b),
            _ => false,
        },
    }
}

/// Whether the supported checked scalar conversion can fail for some input.
pub fn checked_conversion_fallible(source: BuiltinType, target: BuiltinType) -> Option<bool> {
    if lossless_from(source, target) {
        Some(false)
    } else if source.integer_layout().is_some() && target.integer_layout().is_some() {
        Some(true)
    } else {
        None
    }
}
