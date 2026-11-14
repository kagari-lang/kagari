//! Decode the current scalar-only native result subset without heap transport.
#[cfg(test)]
mod tests;

use crate::value::carriers::RootedValue;
use kagari_abi::{
    native_call::{JIT_VALUE_TAG_BOOL, JIT_VALUE_TAG_I32, JIT_VALUE_TAG_UNIT, JitValue},
    representation::ValueType,
};

/// The returned representation belongs to this decoded inline value. Callers
/// compare it with the verified entry only after invalid payload rejection;
/// malformed data must not be mistaken for a valid but mismatching signature.
pub(crate) fn decode_native_value(value: JitValue) -> Option<(ValueType, RootedValue)> {
    Some(match value.tag {
        JIT_VALUE_TAG_UNIT => (ValueType::Unit, RootedValue::unit()),
        JIT_VALUE_TAG_BOOL => (ValueType::Bool, RootedValue::from(value.payload != 0)),
        JIT_VALUE_TAG_I32 => (
            ValueType::I32,
            RootedValue::from(i32::try_from(value.payload).ok()?),
        ),
        _ => return None,
    })
}
