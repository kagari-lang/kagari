//! Mandatory Rust implementations of the compiler-owned language foundation.
//! The declaration catalog is the single authority for every signature and bound.
mod hash;
use crate::gc::HeapObjectId;
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        catalog::DeclarationCatalog,
        context::CallContext,
        module::NativeModule,
        scalar::NativeScalar,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{callable::CallableImplementation, language::catalog, operations::IterOp};
use std::collections::BTreeMap;

type Entry = for<'call> fn(&mut CallContext<'call>) -> NativeResult<Value>;

pub fn module() -> NativeResult<NativeModule> {
    let declaration = catalog::declarations();
    let mut bindings = BTreeMap::new();
    for function in declaration.native_declarations() {
        let CallableImplementation::Native(id) = &function.function.implementation else {
            return Err(RuntimeError::metadata_conflict(
                "invalid foundation binding declaration",
            ));
        };
        if bindings.contains_key(id) {
            continue;
        }
        let name = id
            .path
            .last()
            .ok_or_else(|| RuntimeError::metadata_conflict("foundation binding identity"))?
            .name
            .as_str();
        let entry: Entry = match name {
            "$foundation_list_new" => list_new,
            "$foundation_list_len" => list_len,
            "$foundation_list_is_empty" => list_is_empty,
            "$foundation_list_get" => list_get,
            "$foundation_list_index" => list_index,
            "$foundation_list_push" => list_push,
            "$foundation_list_push_fluent" => list_push_fluent,
            "$foundation_list_pop" => list_pop,
            "$foundation_list_insert" => list_insert,
            "$foundation_list_insert_fluent" => list_insert_fluent,
            "$foundation_list_remove" => list_remove,
            "$foundation_list_clear" => list_clear,
            "$foundation_list_clear_fluent" => list_clear_fluent,
            "$foundation_list_set" => list_set,
            "$foundation_list_set_fluent" => list_set_fluent,
            "$foundation_list_iter"
            | "$foundation_map_iter"
            | "$foundation_set_iter"
            | "$foundation_Range_iter"
            | "$foundation_RangeInclusive_iter"
            | "$foundation_RangeFrom_iter" => iter,
            "$foundation_cursor_next" => next,
            "$foundation_map_new" => hash::map_new,
            "$foundation_map_len" => hash::map_len,
            "$foundation_map_is_empty" => hash::map_is_empty,
            "$foundation_map_contains_key" => hash::map_contains,
            "$foundation_map_get" => hash::map_get,
            "$foundation_map_insert" => hash::map_insert,
            "$foundation_map_insert_fluent" => hash::map_insert_fluent,
            "$foundation_map_remove" => hash::map_remove,
            "$foundation_map_clear" => hash::map_clear,
            "$foundation_map_clear_fluent" => hash::map_clear_fluent,
            "$foundation_set_new" => hash::set_new,
            "$foundation_set_len" => hash::set_len,
            "$foundation_set_is_empty" => hash::set_is_empty,
            "$foundation_set_contains" => hash::set_contains,
            "$foundation_set_insert" => hash::set_insert,
            "$foundation_set_insert_fluent" => hash::set_insert_fluent,
            "$foundation_set_remove" => hash::set_remove,
            "$foundation_set_clear" => hash::set_clear,
            "$foundation_set_clear_fluent" => hash::set_clear_fluent,
            "$foundation_Range_start_bound"
            | "$foundation_RangeInclusive_start_bound"
            | "$foundation_RangeFrom_start_bound"
            | "$foundation_RangeTo_start_bound"
            | "$foundation_RangeToInclusive_start_bound"
            | "$foundation_RangeFull_start_bound" => start_bound,
            "$foundation_Range_end_bound"
            | "$foundation_RangeInclusive_end_bound"
            | "$foundation_RangeFrom_end_bound"
            | "$foundation_RangeTo_end_bound"
            | "$foundation_RangeToInclusive_end_bound"
            | "$foundation_RangeFull_end_bound" => end_bound,
            _ => {
                return Err(RuntimeError::metadata_conflict(format!(
                    "missing foundation implementation {name}"
                )));
            }
        };
        bindings.insert(
            id.clone(),
            NativeBinding::new(
                vec![Codec::Value; function.function.params.len()],
                Codec::Value,
                entry,
            ),
        );
    }
    NativeModule::checked(
        declaration,
        bindings.into_iter().collect(),
        BTreeMap::new(),
        &DeclarationCatalog::default(),
    )
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid foundation receiver")
}
fn array(cx: &CallContext<'_>) -> NativeResult<HeapObjectId> {
    let Value::Array(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(id)
}
fn index(cx: &CallContext<'_>, slot: usize) -> NativeResult<usize> {
    usize::decode(cx.argument(slot)?)
}
fn list_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}
fn list_len(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_len(array(cx)?)
        .map(NativeScalar::encode)
        .ok_or_else(invalid)
}
fn list_is_empty(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_len(array(cx)?)
        .map(|len| Value::Bool(len == 0))
        .ok_or_else(invalid)
}
pub(super) fn option(cx: &CallContext<'_>, value: Option<Value>) -> NativeResult<Value> {
    let tag = if value.is_some() {
        EnumTag::OptionSome
    } else {
        EnumTag::OptionNone
    };
    cx.heap()
        .alloc_enum(tag, value.into_iter().collect())
        .map(Value::Enum)
}
fn list_get(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let value = cx.heap().array_get(array(cx)?, index(cx, 1)?);
    option(cx, value)
}
fn list_index(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_get(array(cx)?, index(cx, 1)?)
        .ok_or_else(|| {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "list index is out of bounds",
            )
        })
}
fn list_push(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().array_push(array(cx)?, cx.argument(1)?)?;
    Ok(Value::Unit)
}
fn list_push_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    list_push(cx)?;
    cx.argument(0)
}
fn list_pop(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    cx.heap().ensure_structure_mutable(id)?;
    let length = cx.heap().array_len(id).ok_or_else(invalid)?;
    let value = length
        .checked_sub(1)
        .and_then(|index| cx.heap().array_get(id, index));
    // Allocate the result before committing the write. No callback or safepoint
    // can invalidate this preparation before the removal.
    let result = option(cx, value)?;
    cx.heap().array_pop(id)?;
    Ok(result)
}
fn list_insert(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_insert(array(cx)?, index(cx, 1)?, cx.argument(2)?)?;
    Ok(Value::Unit)
}
fn list_insert_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    list_insert(cx)?;
    cx.argument(0)
}
fn list_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    let index = index(cx, 1)?;
    cx.heap().ensure_structure_mutable(id)?;
    let result = option(cx, cx.heap().array_get(id, index))?;
    cx.heap().array_remove(id, index)?;
    Ok(result)
}
fn list_clear(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap().array_clear(array(cx)?)?;
    Ok(Value::Unit)
}
fn list_clear_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    list_clear(cx)?;
    cx.argument(0)
}
fn list_set(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.heap()
        .array_set(array(cx)?, index(cx, 1)?, cx.argument(2)?)?;
    Ok(Value::Unit)
}
fn list_set_fluent(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    list_set(cx)?;
    cx.argument(0)
}
fn iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.runtime.iter_operation(
        cx.owner(),
        &cx.argument(0)?,
        cx.argument_type(0)?,
        IterOp::New,
    )
}
fn next(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.runtime.iter_operation(
        cx.owner(),
        &cx.argument(0)?,
        cx.argument_type(0)?,
        IterOp::Next,
    )
}
fn start_bound(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(cx, false)
}
fn end_bound(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(cx, true)
}
fn range_bound(cx: &mut CallContext<'_>, upper: bool) -> NativeResult<Value> {
    let Value::Range(range) = cx.argument(0)? else {
        return Err(invalid());
    };
    range.bound(cx.heap(), cx.argument_type(0)?, cx.result_type(), upper)
}
