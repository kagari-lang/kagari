//! Concrete Vec operations borrow call roots; callback algorithms retain SDK handles.
use crate::foundation::{
    arrays::operations::{index_argument, invalid as invalid_array, source as array_source},
    enumerations::value::{EnumSymbols, option},
    iteration::traversal::for_each,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::HeapObjectId,
    native::{
        binding::{NativeBinding, NativeResult},
        collections::vector::ScriptVec,
        context::CallContext,
        declarations::SelectedCall,
        function_handle::PinnedFunction,
        scalar::NativeScalar,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_types::{declaration::NativeDeclaration, ty::Ty};

type Vector = ScriptVec<ScriptValue>;

pub(crate) fn clear(declaration: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::declared(
        declaration,
        |cx: &mut NativeContext<'_>, (values,): (Vector,)| values.clear(cx),
    )
}

pub(crate) fn clear_fluent(declaration: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::declared(
        declaration,
        |cx: &mut NativeContext<'_>, (values,): (Vector,)| {
            values.clear(cx)?;
            Ok(values)
        },
    )
}

pub(crate) fn retain(declaration: &DefinitionMetadata<NativeDeclaration>) -> NativeBinding {
    NativeBinding::declared(
        declaration,
        |cx: &mut NativeContext<'_>,
         (values, predicate): (Vector, PinnedFunction<(ScriptValue,), bool>)| {
            values.retain(cx, |cx, value| predicate.call(cx, (value,)))
        },
    )
}

pub(crate) fn dedup(
    declaration: &DefinitionMetadata<NativeDeclaration>,
) -> NativeResult<NativeBinding> {
    let [requirement] = declaration.records().callable_requirements.as_slice() else {
        return Err(RuntimeError::metadata_conflict("Vec dedup requirement"));
    };
    let selected = SelectedCall::from_declaration(declaration, requirement)?;
    Ok(NativeBinding::declared(
        declaration,
        move |cx: &mut NativeContext<'_>, (values,): (Vector,)| {
            let equal = cx.selected_method::<(ScriptValue, ScriptValue), bool>(&selected)?;
            values.dedup_by(cx, |cx, left, right| equal.call(cx, (left, right)))
        },
    ))
}

pub(crate) fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.sequence_argument(0)?;
    cx.heap()
        .sequence_len(id)
        .map(NativeScalar::encode)
        .ok_or_else(|| RuntimeError::module_validation("Vec handle length"))
}

pub(crate) fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let length = u64::decode(length(cx)?)?;
    Ok(Value::Bool(length == 0))
}

fn element(cx: &CallContext<'_>) -> NativeResult<Option<Value>> {
    let id = cx.sequence_argument(0)?;
    cx.heap().sequence_element(id, index_argument(cx, 1)?)
}

pub(crate) fn get(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    option(symbols, cx, element(cx)?)
}

pub(crate) fn insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.sequence_argument(0)?;
    let index = index_argument(cx, 1)?;
    cx.ensure_collection_mutable(id)?;
    let length = cx
        .heap()
        .sequence_len(id)
        .ok_or_else(|| RuntimeError::module_validation("Vec handle length"))?;
    let position = usize::try_from(index)
        .ok()
        .filter(|position| *position <= length)
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                format!("Vec insert index {index} exceeds length {length}"),
            )
        })?;
    let value = cx.checked_argument(2)?;
    cx.heap().sequence_insert(id, position, value)?;
    Ok(Value::Unit)
}

pub(crate) fn insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    insert(cx)?;
    cx.argument(0)
}

pub(crate) fn push(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.sequence_argument(0)?;
    let value = cx.checked_argument(1)?;
    cx.heap().sequence_push(id, value)?;
    Ok(Value::Unit)
}

pub(crate) fn push_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    push(cx)?;
    cx.argument(0)
}

pub(crate) fn from_array(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let source = array_source(cx)?;
    let count = cx.heap().array_len(source).ok_or_else(invalid_array)?;
    let result = cx.allocate_result()?;
    let _root = cx.heap().root_value(result).ok_or_else(invalid_array)?;
    let Value::GcHandle(target) = result else {
        return Err(invalid_array());
    };
    for index in 0..count {
        cx.poll()?;
        let value = cx
            .heap()
            .array_get(source, index)
            .ok_or_else(invalid_array)?;
        cx.heap().sequence_push(target, value)?;
    }
    Ok(result)
}

pub(crate) fn from_iter(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Ty::NativeObject(_) = cx.result_type() else {
        return Err(invalid_construction());
    };
    let result = cx.allocate_result()?;
    let _root = cx
        .heap()
        .root_value(result)
        .ok_or_else(invalid_construction)?;
    let Value::GcHandle(id) = result else {
        return Err(invalid_construction());
    };
    for_each(symbols, cx, 0, |cx, value| {
        cx.heap().sequence_push(id, value)
    })?;
    Ok(result)
}

fn invalid_construction() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

pub(crate) fn new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid foundation receiver")
}

fn array(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::GcHandle(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(id)
}

pub(crate) fn pop(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    cx.ensure_collection_mutable(id)?;
    let length = cx.heap().sequence_len(id).ok_or_else(invalid)?;
    let value = length
        .checked_sub(1)
        .and_then(|index| cx.heap().sequence_get(id, index));
    // Allocate the result before committing the write. No callback or safepoint
    // can invalidate this preparation before the removal.
    let result = option(symbols, cx, value)?;
    cx.heap().sequence_pop(id)?;
    Ok(result)
}

pub(crate) fn remove(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    let index = usize::try_from(index_argument(cx, 1)?).ok();
    cx.ensure_collection_mutable(id)?;
    let result = option(
        symbols,
        cx,
        index.and_then(|index| cx.heap().sequence_get(id, index)),
    )?;
    if let Some(index) = index {
        cx.heap().sequence_remove(id, index)?;
    }
    Ok(result)
}
