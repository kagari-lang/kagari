//! Cold scalar admission/materialization. Hot kernels consume complete payloads.
use crate::value::Value;
use kagari_abi::representation::ValueType;
use kagari_types::scalar::BuiltinType;

pub(crate) fn encode(value: &Value) -> Option<u64> {
    Some(match *value {
        Value::Unit => 0,
        Value::Bool(v) => u64::from(v),
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
        ValueType::I32 => Value::I32(bits as i32),
        ValueType::I64 => Value::I64(bits as i64),
        ValueType::U64 => Value::U64(bits),
        ValueType::F32 => Value::F32(f32::from_bits(bits as u32)),
        ValueType::F64 => Value::F64(f64::from_bits(bits)),
        _ => return None,
    })
}

/// Shared scalar domain admission for banks and type-directed field storage.
pub(crate) fn admits_payload(
    representation: ValueType,
    semantic: Option<BuiltinType>,
    bits: u64,
) -> bool {
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
