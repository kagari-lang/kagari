//! Concrete Vec operations borrow call roots; callback algorithms retain SDK handles.
use crate::bindings::{Entry, option};
use kagari_runtime::{
    error::RuntimeError,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        collections::vector::ScriptVec,
        context::CallContext,
        declarations::SelectedCall,
        function_handle::PinnedFunction,
        primitive::NativePrimitive,
        scalar::NativeScalar,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
    value::Value,
};
use kagari_types::{declaration::NativeDeclaration, scalar::BuiltinType, ty::Ty};

type Vector = ScriptVec<ScriptValue>;

pub(super) fn binding(
    name: &str,
    declaration: &NativeDeclaration,
) -> NativeResult<Option<NativeBinding>> {
    Ok(Some(match name {
        "$foundation_list_len" => scoped(vec![Codec::Sequence], length),
        "$foundation_list_is_empty" => scoped(vec![Codec::Sequence], is_empty),
        "$foundation_list_get" => scoped(
            vec![
                Codec::Sequence,
                Codec::Scalar(Ty::Builtin(BuiltinType::USize)),
            ],
            get,
        ),
        "$foundation_list_index" => NativeBinding::primitive(NativePrimitive::VecIndex),
        "$foundation_list_push" => scoped(vec![Codec::MutableSequence, Codec::Value], push),
        "$foundation_list_push_fluent" => {
            scoped(vec![Codec::MutableSequence, Codec::Value], push_fluent)
        }
        "$foundation_list_insert" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index, value): (Vector, usize, ScriptValue)| {
                values.insert(cx, index, value)
            },
        ),
        "$foundation_list_insert_fluent" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index, value): (Vector, usize, ScriptValue)| {
                values.insert(cx, index, value)?;
                Ok(values)
            },
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
            let [requirement] = declaration.callable_requirements.as_slice() else {
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
    let id = cx.array_argument(0, false)?;
    cx.heap()
        .array_len(id)
        .map(NativeScalar::encode)
        .ok_or_else(|| RuntimeError::module_validation("array handle length"))
}

fn is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let length = usize::decode(length(cx)?)?;
    Ok(Value::Bool(length == 0))
}

fn element(cx: &CallContext<'_>) -> NativeResult<Option<Value>> {
    let id = cx.array_argument(0, false)?;
    let index = usize::decode(cx.argument(1)?)?;
    cx.heap().array_element(id, index)
}

fn get(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    option(cx, element(cx)?)
}

fn push(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = cx.array_argument(0, true)?;
    let value = cx.checked_argument(1)?;
    cx.heap().array_push(id, value)?;
    Ok(Value::Unit)
}

fn push_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    push(cx)?;
    cx.argument(0)
}
