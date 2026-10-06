//! Cold scalar admission/materialization. Hot kernels consume complete payloads.
use crate::value::Value;
use kagari_abi::representation::ValueType;

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
