//! Shared Vec mutations use retained handles and contextual generic conversion.
use kagari_runtime::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{NativeBinding, NativeResult},
        collections::vector::ScriptVec,
        declarations::SelectedCall,
        function_handle::PinnedFunction,
        typed::NativeContext,
        value_handle::ScriptValue,
    },
};
use kagari_types::declaration::NativeDeclaration;

type Vector = ScriptVec<ScriptValue>;

pub(super) fn binding(
    name: &str,
    declaration: &NativeDeclaration,
) -> NativeResult<Option<NativeBinding>> {
    Ok(Some(match name {
        "$foundation_list_len" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values,): (Vector,)| values.len(cx),
        ),
        "$foundation_list_is_empty" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values,): (Vector,)| values.is_empty(cx),
        ),
        "$foundation_list_get" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index): (Vector, usize)| values.get(cx, index),
        ),
        "$foundation_list_index" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index): (Vector, usize)| {
                values.get(cx, index)?.ok_or_else(|| {
                    RuntimeError::new(
                        RuntimeErrorKind::IndexOutOfBounds,
                        "list index is out of bounds",
                    )
                })
            },
        ),
        "$foundation_list_push" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, value): (Vector, ScriptValue)| {
                values.push(cx, value)
            },
        ),
        "$foundation_list_push_fluent" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, value): (Vector, ScriptValue)| {
                values.push(cx, value)?;
                Ok(values)
            },
        ),
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
        "$foundation_list_set" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index, value): (Vector, usize, ScriptValue)| {
                values.set(cx, index, value)
            },
        ),
        "$foundation_list_set_fluent" => NativeBinding::declared(
            declaration,
            |cx: &mut NativeContext<'_>, (values, index, value): (Vector, usize, ScriptValue)| {
                values.set(cx, index, value)?;
                Ok(values)
            },
        ),
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
