//! Shared ordered construction traversal with the standard Vec fast path.
use crate::{
    collections::vector::ScriptVec,
    foundation::enumerations::value::{self as enums, EnumSymbols},
    identity,
};
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{binding::NativeResult, context::CallContext, value_handle::ScriptValue},
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
    if argument.definitions().lookup(identity::vector_id()) != Some(nominal.declaration) {
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
    if vec_argument(cx)?.is_some() {
        let vector: ScriptVec<ScriptValue> = {
            let mut native = cx.native_context()?;
            native
                .conversion()
                .decode_value(&cx.argument_type_argument(0)?, &source)?
        };
        let (revision, length) = {
            let native = cx.native_context()?;
            (vector.revision(&native)?, vector.len(&native)?)
        };
        for index in 0..length {
            cx.poll()?;
            let value = {
                let mut native = cx.native_context()?;
                vector.check_revision(&native, revision)?;
                vector.get(&mut native, index)?.ok_or_else(invalid)?
            };
            let raw = {
                let mut native = cx.native_context()?;
                let ty = value.type_argument().clone();
                native.conversion().encode_value(&ty, value.clone())?
            };
            visit(cx, raw)?;
        }
        return vector.check_revision(&cx.native_context()?, revision);
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
