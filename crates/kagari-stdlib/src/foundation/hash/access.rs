//! Call-scoped hash entrypoints reuse the provider kernel without owning receivers.
use crate::{
    collections::{
        hash::{HashPayload, input::Input, operations},
        map::MapStorage,
        set::SetStorage,
    },
    foundation::enumerations::value::{EnumSymbols, option},
};
use kagari_runtime::{
    native::{binding::NativeResult, context::CallContext, value_handle::ScriptValue},
    value::Value,
};

pub(crate) fn len<P: HashPayload>(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<P>(0)?;
    let cx = call.native_context()?;
    Ok(Value::U64(operations::len(&cx, &object)? as u64))
}

pub(crate) fn is_empty<P: HashPayload>(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<P>(0)?;
    let cx = call.native_context()?;
    Ok(Value::Bool(operations::len(&cx, &object)? == 0))
}

pub(crate) fn contains<P: HashPayload>(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<P>(0)?;
    let mut cx = call.native_context()?;
    let key = Input::Argument(call, 1);
    let found = operations::lookup(&mut cx, &object, key)?;
    Ok(Value::Bool(found.slot.is_some()))
}

pub(crate) fn map_get(symbols: &EnumSymbols, call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<MapStorage>(0)?;
    let mut cx = call.native_context()?;
    let key = Input::Argument(call, 1);
    // SAFETY: The receiver's native-call root protects extraction through Option
    // publication. No user callback runs between the completed read and allocation.
    let value = unsafe {
        operations::lookup_read(&mut cx, &object, key, |table, slot, _| {
            Ok(slot.map(|slot| table.slots[slot].value.value_transport()))
        })?
    };
    option(symbols, call, value)
}

pub(crate) fn map_insert(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<MapStorage>(0)?;
    let mut cx = call.native_context()?;
    let key = Input::Argument(call, 1);
    let value = Input::Argument(call, 2);
    operations::insert(&mut cx, &object, key, Some(value))?;
    Ok(Value::Unit)
}

pub(crate) fn map_insert_fluent(call: &mut CallContext<'_>) -> NativeResult<Value> {
    map_insert(call)?;
    // SAFETY: The call frame retains the receiver through return publication.
    unsafe { call.argument_transport(0) }
}

pub(crate) fn set_insert(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<SetStorage>(0)?;
    let mut cx = call.native_context()?;
    let key = Input::Argument(call, 1);
    operations::insert(&mut cx, &object, key, None)?;
    Ok(Value::Unit)
}

pub(crate) fn set_insert_fluent(call: &mut CallContext<'_>) -> NativeResult<Value> {
    set_insert(call)?;
    // SAFETY: The call frame retains the receiver through return publication.
    unsafe { call.argument_transport(0) }
}

pub(crate) fn map_remove(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<MapStorage>(0)?;
    let mut cx = call.native_context()?;
    operations::ensure_mutable(&mut cx, &object)?;
    let key = Input::Argument(call, 1);
    let found = operations::lookup(&mut cx, &object, key)?;
    let value = operations::get(&cx, &object, &found)?;
    // Allocate and retain Option before committing removal. Failed preparation
    // leaves the entry installed and does not retry any completed Hash/Eq callback.
    let result_type = call.result_type_argument()?;
    let result: ScriptValue = {
        let mut conversion = cx.conversion_scope(cx.owner())?;
        conversion.encode_value(&result_type, value)?
    };
    operations::remove(&mut cx, &object, found)?;
    // SAFETY: The prepared result survives removal; this terminal Native return
    // publishes it before another collection can run.
    unsafe { result.value_transport(cx.runtime()) }
}

pub(crate) fn set_remove(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<SetStorage>(0)?;
    let mut cx = call.native_context()?;
    operations::ensure_mutable(&mut cx, &object)?;
    let key = Input::Argument(call, 1);
    let found = operations::lookup(&mut cx, &object, key)?;
    Ok(Value::Bool(operations::remove(&mut cx, &object, found)?))
}

pub(crate) fn clear<P: HashPayload>(call: &mut CallContext<'_>) -> NativeResult<Value> {
    let object = call.payload::<P>(0)?;
    let mut cx = call.native_context()?;
    operations::clear(&mut cx, &object)?;
    Ok(Value::Unit)
}

pub(crate) fn clear_fluent<P: HashPayload>(call: &mut CallContext<'_>) -> NativeResult<Value> {
    clear::<P>(call)?;
    // SAFETY: The call frame retains the receiver through return publication.
    unsafe { call.argument_transport(0) }
}
