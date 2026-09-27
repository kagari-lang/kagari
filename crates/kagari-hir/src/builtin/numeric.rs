//! Built-in conversions follow Rust's portable From matrix.
use super::surface::StandardEnum;
use crate::types::{BuiltinType, TypeId};

pub fn lossless_from(source: BuiltinType, target: BuiltinType) -> bool {
    use BuiltinType::*;
    if source == target {
        return source.number_type().is_some() || source == Bool;
    }
    if source == Bool {
        return target.integer_layout().is_some();
    }
    match target {
        F32 => matches!(source, I8 | I16 | U8 | U16),
        F64 => matches!(source, I8 | I16 | I32 | U8 | U16 | U32 | F32),
        ISize => matches!(source, I8 | I16 | U8),
        USize => matches!(source, U8 | U16),
        _ if matches!(source, ISize | USize) => false,
        _ => match (source.integer_layout(), target.integer_layout()) {
            (Some((a, sa)), Some((b, sb))) => (sa == sb && a < b) || (!sa && sb && a < b),
            _ => false,
        },
    }
}

pub fn try_error(source: BuiltinType, target: BuiltinType) -> Option<TypeId> {
    let kind = if lossless_from(source, target) {
        StandardEnum::Infallible
    } else if source.integer_layout().is_some() && target.integer_layout().is_some() {
        StandardEnum::TryFromIntError
    } else {
        return None;
    };
    Some(TypeId::StandardEnum { kind, args: vec![] })
}
