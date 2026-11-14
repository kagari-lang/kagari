//! Canonical scalar payloads and checked semantic integer domains.
use kagari_abi::representation::ValueType;
use kagari_types::scalar::BuiltinType;

pub(crate) fn decode_char(bits: u64) -> Option<char> {
    char::from_u32(u32::try_from(bits).ok()?)
}

/// Shared scalar domain admission for banks and type-directed field storage.
pub(crate) fn admits_payload(
    representation: ValueType,
    semantic: Option<BuiltinType>,
    bits: u64,
) -> bool {
    if representation == ValueType::Char || semantic == Some(BuiltinType::Char) {
        return representation == ValueType::Char
            && semantic.is_none_or(|ty| ty == BuiltinType::Char)
            && decode_char(bits).is_some();
    }
    if representation == ValueType::Bool && bits > 1 {
        return false;
    }
    let Some((min, max)) = semantic.and_then(BuiltinType::integer_bounds) else {
        return true;
    };
    let value = match representation {
        ValueType::I32 | ValueType::I64 => i128::from(bits as i64),
        ValueType::U64 => i128::from(bits),
        _ => return false,
    };
    value >= min && value <= max
}
