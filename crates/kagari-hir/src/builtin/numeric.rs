//! Built-in conversions follow Rust's portable From matrix.
use super::surface::StandardEnum;
use crate::types::TypeId;
use kagari_abi::scalar::BuiltinType;

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
