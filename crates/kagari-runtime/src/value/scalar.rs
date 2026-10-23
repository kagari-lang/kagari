//! Cold scalar admission/materialization. Hot kernels consume complete payloads.
use crate::value::Value;
use kagari_abi::representation::ValueType;
use kagari_types::scalar::BuiltinType;

pub(crate) fn encode(value: &Value) -> Option<u64> {
    Some(match *value {
        Value::Unit => 0,
        Value::Bool(v) => u64::from(v),
        Value::Char(v) => u64::from(u32::from(v)),
        Value::I32(v) => v as i64 as u64,
        Value::I64(v) => v as u64,
        Value::U64(v) => v,
        Value::F32(v) => u64::from(v.to_bits()),
        Value::F64(v) => v.to_bits(),
        _ => return None,
    })
}

pub(crate) fn decode(ty: ValueType, bits: u64) -> Option<Value> {
    Some(match ty {
        ValueType::Unit => Value::Unit,
        ValueType::Bool if bits <= 1 => Value::Bool(bits != 0),
        ValueType::Char => Value::Char(decode_char(bits)?),
        ValueType::I32 => Value::I32(bits as i32),
        ValueType::I64 => Value::I64(bits as i64),
        ValueType::U64 => Value::U64(bits),
        ValueType::F32 => Value::F32(f32::from_bits(bits as u32)),
        ValueType::F64 => Value::F64(f64::from_bits(bits)),
        _ => return None,
    })
}

fn decode_char(bits: u64) -> Option<char> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_payloads_require_canonical_unicode_scalars_and_exact_domains() {
        for ch in ['\0', 'a', 'é', '\u{d7ff}', '\u{e000}', '🙂', char::MAX] {
            let value = Value::Char(ch);
            let bits = encode(&value).unwrap();
            assert_eq!(decode(ValueType::Char, bits), Some(value));
            assert!(admits_payload(
                ValueType::Char,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(admits_payload(ValueType::Char, None, bits));
            assert!(!admits_payload(
                ValueType::I64,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(!admits_payload(
                ValueType::Char,
                Some(BuiltinType::U32),
                bits
            ));
            assert!(!value.has_representation(ValueType::I64));
            assert!(!Value::I64(bits as i64).has_representation(ValueType::Char));
        }
        for bits in [0xd800, 0xdfff, 0x110000, 0x1_0000_0061, u64::MAX] {
            assert_eq!(decode(ValueType::Char, bits), None);
            assert!(!admits_payload(
                ValueType::Char,
                Some(BuiltinType::Char),
                bits
            ));
            assert!(!admits_payload(ValueType::Char, None, bits));
        }
    }
}
