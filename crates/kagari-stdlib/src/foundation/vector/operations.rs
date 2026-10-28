//! Concrete Vec operations borrow call roots; callback algorithms retain SDK handles.
use crate::{
    collections::vector::{
        ScriptVec, VecStorage, borrowed,
        cursor::{self, VecCursor},
    },
    foundation::{
        arrays::operations::{index_argument, source as array_source},
        enumerations::value::{EnumSymbols, converted_option, option},
        iteration::traversal::for_each,
    },
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        function_handle::Function,
        scalar::NativeScalar,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_types::declaration::NativeDeclaration;

type Vector = ScriptVec<ScriptValue>;

pub(crate) fn clear(_: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::new([Codec::Value], Codec::Value, |cx| {
        borrowed::clear(cx)?;
        Ok(Value::Unit)
    })
}

pub(crate) fn clear_fluent(_: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::new([Codec::Value], Codec::Value, |cx| {
        borrowed::clear(cx)?;
        cx.argument(0)
    })
}

pub(crate) fn retain(declaration: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::declared(
        declaration,
        |cx: &mut NativeContext<'_>,
         (values, predicate): (Vector, Function<(ScriptValue,), bool>)| {
            let predicate = predicate.prepare_call(cx.runtime())?;
            values.retain_retained(cx, |cx, value| {
                predicate.call_with_options(cx, (value,), cx.runtime().execution_options())
            })
        },
    )
}

pub(crate) fn dedup(
    declaration: &DefinitionMetadata<NativeDeclaration>,
) -> NativeResult<NativeBinding> {
    let [_requirement] = declaration.records().callable_requirements.as_slice() else {
        return Err(RuntimeError::metadata_conflict("Vec dedup requirement"));
    };
    Ok(NativeBinding::declared(
        declaration,
        move |cx: &mut NativeContext<'_>, (values,): (Vector,)| {
            let equal = cx.selected_function::<(ScriptValue, ScriptValue), bool>(0)?;
            let equal = equal.prepare_call(cx.runtime())?;
            values.dedup_retained(cx, |cx, left, right| {
                equal.call_with_options(cx, (left, right), cx.runtime().execution_options())
            })
        },
    ))
}

pub(crate) fn next(symbols: &EnumSymbols, call: &mut CallContext<'_>) -> NativeResult<Value> {
    let mut cx = call.native_context()?;
    let result_type = call.result_type_argument()?;
    // The binding proves outer Option identity/arity; validate this call's child
    // mapping before the cursor can advance, including when it returns None.
    let element = cx.conversion().parameter(&result_type, 0)?;
    cx.conversion().check_type::<ScriptValue>(&element)?;
    let receiver = call.payload::<VecCursor>(0)?;
    let result = cursor::next(&mut cx, &receiver)?;
    // Advancement commits before fallible encoding; retained items protect the
    // result across conversion safepoints after the provider loans have ended.
    converted_option(symbols, call, cx.conversion(), &result_type, result)
}

pub(crate) fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: Only scalar length leaves the call-rooted shared payload loan.
    unsafe { cx.with_payload::<VecStorage, _>(0, |v| Ok(v.len().encode())) }
}

pub(crate) fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: Only scalar state leaves the call-rooted shared payload loan.
    unsafe { cx.with_payload::<VecStorage, _>(0, |v| Ok(Value::Bool(v.len() == 0))) }
}

pub(crate) fn get(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let index = usize::try_from(index_argument(cx, 1)?).ok();
    // SAFETY: The source stays call-rooted through Option allocation, and no
    // callback or mutation occurs between extraction and result publication.
    let value = unsafe {
        cx.with_payload::<VecStorage, _>(0, |v| Ok(index.and_then(|index| v.get(index))))?
    };
    option(symbols, cx, value)
}

pub(crate) fn index(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let index = usize::try_from(index_argument(cx, 1)?).map_err(|_| bounds())?;
    // SAFETY: Raw native result handoff runs no callback or safepoint after
    // extraction; the invoking frame publishes the result while arguments live.
    unsafe { cx.with_payload::<VecStorage, _>(0, |v| v.get(index).ok_or_else(bounds)) }
}

pub(crate) fn insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let index = usize::try_from(index_argument(cx, 1)?).map_err(|_| bounds())?;
    borrowed::insert(cx, Some(index), 2)?;
    Ok(Value::Unit)
}

pub(crate) fn insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    insert(cx)?;
    cx.argument(0)
}

pub(crate) fn push(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    borrowed::insert(cx, None, 1)?;
    Ok(Value::Unit)
}

pub(crate) fn push_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    push(cx)?;
    cx.argument(0)
}

pub(crate) fn set(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let index = usize::try_from(index_argument(cx, 1)?).map_err(|_| bounds())?;
    borrowed::set(cx, index, 2)?;
    Ok(Value::Unit)
}

pub(crate) fn set_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    set(cx)?;
    cx.argument(0)
}

pub(crate) fn new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result_payload(&[], |cx, result| {
        VecStorage::empty_element(cx.type_parameter(result, 0)?, cx.owner())
    })
}

fn result(cx: &CallContext<'_>, value: Value) -> NativeResult<Vector> {
    let argument = cx.result_type_argument()?;
    cx.native_context()?
        .conversion()
        .decode_value(&argument, &value)
}

pub(crate) fn from_array(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result_payload(&[], |cx, result| {
        let source = array_source(cx)?;
        VecStorage::from_array(cx, source, result)
    })
}

pub(crate) fn from_iter(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let value = new(cx)?;
    let target = result(cx, value)?;
    let element = cx.result_type_parameter(0)?;
    for_each(symbols, cx, 0, |cx, value| {
        let mut native = cx.native_context()?;
        let item = native.conversion().decode_value(&element, &value)?;
        target.push(&mut native, item)
    })?;
    Ok(value)
}

pub(crate) fn pop(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    // SAFETY: Only scalar length leaves the call-rooted payload loan.
    let count = unsafe { cx.with_payload::<VecStorage, _>(0, |v| Ok(v.len()))? };
    remove_at(symbols, cx, Some(count.saturating_sub(1)))
}

pub(crate) fn remove(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let index = usize::try_from(index_argument(cx, 1)?).ok();
    remove_at(symbols, cx, index)
}

fn remove_at(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    index: Option<usize>,
) -> NativeResult<Value> {
    // SAFETY: The call roots the source across Option allocation; the allocated
    // Option then retains the removed edge through mutation and result handoff.
    let item = unsafe {
        cx.with_payload::<VecStorage, _>(0, |v| Ok(index.and_then(|index| v.get(index))))?
    };
    let result = option(symbols, cx, item)?;
    let _result = cx
        .heap()
        .root_value(result)
        .ok_or_else(|| RuntimeError::module_validation("Vec removal result retention"))?;
    if let Some(index) = index {
        borrowed::remove(cx, index)?;
    }
    Ok(result)
}

fn bounds() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::IndexOutOfBounds,
        "Vec index is out of bounds",
    )
}
