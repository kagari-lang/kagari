//! Checked scalar protocol operations shared by ordinary native providers.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native_value::{NativeCall, NativeResult, NativeValue, invalid},
    value::{MapKey, Value},
    value_semantics::format_value,
};
use kagari_abi::{standard::surface::StandardEnum, types::AbiType};

fn receiver<T: NativeValue + Clone>(call: &NativeCall, value: &T) -> NativeResult<Value> {
    let expected = call.signature.params.first().ok_or_else(invalid)?;
    if !matches!(
        expected,
        AbiType::Builtin(_)
            | AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                ..
            }
    ) {
        return Err(invalid());
    }
    let value = value.clone().write(call, expected)?;
    call.check(&value, expected)?;
    call.charge_work(match &value {
        Value::Str(text) => text.len() as u64,
        _ => 1,
    })?;
    Ok(value)
}

/// Hash a checked scalar first argument using the script key representation.
/// This does not select user-defined protocols or traverse arbitrary objects.
pub fn hash_scalar<T: NativeValue + Clone>(call: &NativeCall, value: &T) -> NativeResult<i64> {
    let value = receiver(call, value)?;
    MapKey::from_value(&call.heap, &value)
        .map(|key| key.script_hash())
        .ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "scalar has no hash semantics")
        })
}

/// Format a checked scalar first argument with the language's bounded formatter.
/// Input and output-byte work is charged, including escaped Debug output.
pub fn format_scalar<T: NativeValue + Clone>(
    call: &NativeCall,
    value: &T,
    debug: bool,
) -> NativeResult<String> {
    let value = receiver(call, value)?;
    let text = format_value(&call.heap, &value, debug)?;
    call.charge_work(text.len() as u64)?;
    Ok(text)
}
