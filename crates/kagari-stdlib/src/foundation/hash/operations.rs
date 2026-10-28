//! Hash callbacks use library-owned storage through the public provider boundary.
use crate::collections::{
    hash::{
        cursor::{self, HashCursor},
        input::Input,
        invalid, operations,
    },
    map::MapStorage,
    set::SetStorage,
};
use crate::foundation::enumerations::value::{EnumSymbols, converted_option};
use kagari_runtime::native::{
    binding::NativeResult, context::CallContext, conversion::context::ConversionContext,
    payload::Opaque, typed::NativeContext, value_handle::ScriptValue,
};
use kagari_runtime::value::Value;

pub(crate) fn map_new(cx: &mut NativeContext<'_>, (): ()) -> NativeResult<Opaque<MapStorage>> {
    let ty = cx.result_native_type::<MapStorage>()?;
    let payload = MapStorage::empty(cx, &ty)?;
    ty.create(cx, payload, &[0, 1])
}

pub(crate) fn set_new(cx: &mut NativeContext<'_>, (): ()) -> NativeResult<Opaque<SetStorage>> {
    let ty = cx.result_native_type::<SetStorage>()?;
    let payload = SetStorage::empty(cx, &ty)?;
    ty.create(cx, payload, &[0, 1])
}

pub(crate) fn map_next(symbols: &EnumSymbols, call: &mut CallContext<'_>) -> NativeResult<Value> {
    let mut cx = call.native_context()?;
    let result_type = call.result_type_argument()?;
    // The linked Option contract does not prove the tuple or its child mappings.
    let element = cx.conversion().parameter(&result_type, 0)?;
    cx.conversion()
        .check_type::<(ScriptValue, ScriptValue)>(&element)?;
    let receiver = call.payload::<HashCursor<MapStorage>>(0)?;
    let result = cursor::map_next(&mut cx, &receiver)?;
    // The ordinary child converter retains the tuple through Option allocation.
    converted_option(symbols, call, cx.conversion(), &result_type, result)
}

pub(crate) fn set_next(symbols: &EnumSymbols, call: &mut CallContext<'_>) -> NativeResult<Value> {
    let mut cx = call.native_context()?;
    let result_type = call.result_type_argument()?;
    let element = cx.conversion().parameter(&result_type, 0)?;
    cx.conversion().check_type::<ScriptValue>(&element)?;
    let receiver = call.payload::<HashCursor<SetStorage>>(0)?;
    let result = cursor::set_next(&mut cx, &receiver)?;
    converted_option(symbols, call, cx.conversion(), &result_type, result)
}

pub(crate) fn from_array(
    cx: &mut NativeContext<'_>,
    (source,): (ScriptValue,),
) -> NativeResult<Opaque<SetStorage>> {
    let argument = source.type_argument().clone();
    let owner = cx.owner().clone();
    let mut conversion = ConversionContext::new(cx.runtime(), &owner)?;
    let element = conversion.parameter(&argument, 0)?;
    let Value::Array(id) = conversion.encode_value(&argument, source)? else {
        return Err(invalid());
    };
    let count = cx.runtime().gc().array_len(id).ok_or_else(invalid)?;
    let result = set_new(cx, ())?;
    for index in 0..count {
        cx.poll()?;
        let value = cx.runtime().gc().array_get(id, index).ok_or_else(invalid)?;
        let key = conversion.decode_value(&element, &value)?;
        let object = result.borrow(cx)?;
        operations::insert(cx, &object, Input::Retained(&key), None)?;
    }
    Ok(result)
}

pub(crate) fn map_from_iter(
    cx: &mut NativeContext<'_>,
    (source,): (ScriptValue,),
) -> NativeResult<Opaque<MapStorage>> {
    let result = map_new(cx, ())?;
    let insert = cx
        .selected_function::<(Opaque<MapStorage>, ScriptValue, ScriptValue), ()>(2)?
        .prepare_call(cx.runtime())?;
    for_each(cx, source, |cx, item| {
        let (key, value) = item.decode::<(ScriptValue, ScriptValue)>(cx)?;
        insert.call_with_options(
            cx,
            (result.clone(), key, value),
            cx.runtime().execution_options(),
        )
    })?;
    Ok(result)
}

pub(crate) fn set_from_iter(
    cx: &mut NativeContext<'_>,
    (source,): (ScriptValue,),
) -> NativeResult<Opaque<SetStorage>> {
    let result = set_new(cx, ())?;
    let insert = cx
        .selected_function::<(Opaque<SetStorage>, ScriptValue), ()>(2)?
        .prepare_call(cx.runtime())?;
    for_each(cx, source, |cx, item| {
        insert.call_with_options(cx, (result.clone(), item), cx.runtime().execution_options())
    })?;
    Ok(result)
}

fn for_each(
    cx: &mut NativeContext<'_>,
    source: ScriptValue,
    mut visit: impl FnMut(&mut NativeContext<'_>, ScriptValue) -> NativeResult<()>,
) -> NativeResult<()> {
    let iter = cx
        .selected_function::<(ScriptValue,), ScriptValue>(3)?
        .prepare_call(cx.runtime())?;
    let next = cx
        .selected_function::<(ScriptValue,), Option<ScriptValue>>(4)?
        .prepare_call(cx.runtime())?;
    let cursor: ScriptValue =
        iter.call_with_options(cx, (source,), cx.runtime().execution_options())?;
    loop {
        cx.poll()?;
        let Some(item) = next.call_with_options::<_, Option<ScriptValue>>(
            cx,
            (cursor.clone(),),
            cx.runtime().execution_options(),
        )?
        else {
            return Ok(());
        };
        visit(cx, item)?;
    }
}
