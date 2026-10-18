//! Concrete Vec operations borrow call roots; callback algorithms retain SDK handles.
use crate::bindings::{Entry, index_argument, option};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        collections::vector::ScriptVec,
        context::CallContext,
        declarations::SelectedCall,
        function_handle::PinnedFunction,
        primitive::NativePrimitive,
        scalar::NativeScalar,
        typed::NativeContext,
        types::Type,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_types::declaration::NativeDeclaration;

type Vector = ScriptVec<ScriptValue>;

pub(super) fn binding(
    name: &str,
    declaration: &DefinitionMetadata<NativeDeclaration>,
) -> NativeResult<Option<NativeBinding>> {
    Ok(Some(match name {
        "$foundation_list_len" => scoped(vec![Codec::Sequence], length),
        "$foundation_list_is_empty" => scoped(vec![Codec::Sequence], is_empty),
        "$foundation_list_get" => scoped(vec![Codec::Sequence, Codec::Exact(Type::u64())], get),
        "$foundation_list_index" => NativeBinding::primitive(NativePrimitive::VecIndex),
        "$foundation_list_push" => scoped(vec![Codec::MutableSequence, Codec::Value], push),
        "$foundation_list_push_fluent" => {
            scoped(vec![Codec::MutableSequence, Codec::Value], push_fluent)
        }
        "$foundation_list_insert" => scoped(
            vec![
                Codec::MutableSequence,
                Codec::Exact(Type::u64()),
                Codec::Value,
            ],
            insert,
        ),
        "$foundation_list_insert_fluent" => scoped(
            vec![
                Codec::MutableSequence,
                Codec::Exact(Type::u64()),
                Codec::Value,
            ],
            insert_fluent,
        ),
        "$foundation_list_clear" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values,): (Vector,)| values.clear(cx),
        ),
        "$foundation_list_clear_fluent" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values,): (Vector,)| {
                values.clear(cx)?;
                Ok(values)
            },
        ),
        "$foundation_list_set" => NativeBinding::primitive(NativePrimitive::VecSet),
        "$foundation_list_set_fluent" => NativeBinding::primitive(NativePrimitive::VecSetFluent),
        "$foundation_list_retain" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>,
             (values, predicate): (Vector, PinnedFunction<(ScriptValue,), bool>)| {
                values.retain(cx, |cx, value| predicate.call(cx, (value,)))
            },
        ),
        "$foundation_list_dedup" => {
            let [requirement] = declaration.records().callable_requirements.as_slice() else {
                return Err(RuntimeError::metadata_conflict("Vec dedup requirement"));
            };
            let selected = SelectedCall::from_declaration(declaration, requirement)?;
            NativeBinding::declared(
                declaration,
                move |cx: &mut NativeContext<'_>, (values,): (Vector,)| {
                    let equal =
                        cx.selected_method::<(ScriptValue, ScriptValue), bool>(&selected)?;
                    values.dedup_by(cx, |cx, left, right| equal.call(cx, (left, right)))
                },
            )
        }
        _ => return Ok(None),
    }))
}

fn scoped(arguments: Vec<Codec>, entry: Entry) -> NativeBinding {
    NativeBinding::new(arguments, Codec::Value, entry)
}

fn length(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.sequence_argument(0)?;
    cx.heap()
        .sequence_len(id)
        .map(NativeScalar::encode)
        .ok_or_else(|| RuntimeError::module_validation("Vec handle length"))
}

fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let length = u64::decode(length(cx)?)?;
    Ok(Value::Bool(length == 0))
}

fn element(cx: &CallContext<'_>) -> NativeResult<Option<Value>> {
    let id = cx.sequence_argument(0)?;
    cx.heap().sequence_element(id, index_argument(cx, 1)?)
}

fn get(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    option(cx, element(cx)?)
}

fn insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

fn insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    insert(cx)?;
    cx.argument(0)
}

fn push(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.sequence_argument(0)?;
    let value = cx.checked_argument(1)?;
    cx.heap().sequence_push(id, value)?;
    Ok(Value::Unit)
}

fn push_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    push(cx)?;
    cx.argument(0)
}
