//! Shared ordered construction traversal with the standard Vec fast path.
use crate::{
    foundation::enumerations::value::{self as enums, EnumSymbols},
    identity,
};
use kagari_common::identity::DefinitionKind;
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext},
    value::Value,
};
use kagari_types::ty::Ty;
use std::slice;

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

// Only the standard Vec implementation promises callback-free buffer traversal.
// Other native objects, even those with sequence storage, use their selected iterators.
pub(crate) fn vec_argument(cx: &CallContext<'_>) -> NativeResult<Option<HeapObjectId>> {
    let argument = cx.argument_type_argument(0)?;
    let Ty::NativeObject(nominal) = argument.ty() else {
        return Ok(None);
    };
    let declaration = identity::type_id("Vec", DefinitionKind::AssociatedType);
    if argument.definitions().lookup(&declaration) != Some(nominal.declaration) {
        return Ok(None);
    }
    let Value::GcHandle(id) = cx.argument(0)? else {
        return Err(invalid());
    };
    Ok(Some(id))
}

pub(crate) fn for_each(
    symbols: &EnumSymbols,
    cx: &mut CallContext<'_>,
    first: usize,
    mut visit: impl FnMut(&mut CallContext<'_>, Value) -> NativeResult<()>,
) -> NativeResult<()> {
    let source = cx.argument(0)?;
    // The standard Vec has no iteration callbacks. Read its contiguous storage
    // directly without allocating a cursor or Option for each element.
    if let Some(id) = vec_argument(cx)? {
        let revision = cx.heap().collection_revision(&source).ok_or_else(invalid)?;
        let length = cx.heap().sequence_len(id).ok_or_else(invalid)?;
        for index in 0..length {
            if index % 256 == 0 {
                cx.poll()?;
            }
            cx.heap().check_collection_revision(&source, revision)?;
            visit(cx, cx.heap().sequence_get(id, index).ok_or_else(invalid)?)?;
        }
        return cx.heap().check_collection_revision(&source, revision);
    }
    let iter = cx.selected_at(first)?;
    let next = cx.selected_at(first + 1)?;
    let cursor = cx.call_values(iter, &[source])?;
    let _root = cx.heap().root_value(cursor).ok_or_else(invalid)?;
    loop {
        cx.poll()?;
        let value = cx.call_values(next, slice::from_ref(&cursor))?;
        let Some(item) = enums::read_option(symbols, cx, &value)? else {
            return Ok(());
        };
        let _item = cx.heap().root_value(item).ok_or_else(invalid)?;
        visit(cx, item)?;
    }
}
