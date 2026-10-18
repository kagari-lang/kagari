//! Concrete Vec operations borrow call roots; callback algorithms retain SDK handles.
use crate::bindings::{index_argument, option};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
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
use kagari_types::declaration::NativeDeclaration;

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

pub(crate) fn get(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    option(cx, element(cx)?)
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
