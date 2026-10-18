//! Shared algorithms awaiting SR05 responsibility-based consolidation with their authors.
pub(crate) mod arrays;
pub(crate) mod comparison;
pub(crate) mod construction;
pub(crate) mod enums;
pub(crate) mod hash;
pub(crate) mod iterable;
pub(crate) mod lists;
pub(crate) mod propagation;

pub(crate) mod vectors;
use crate::assembly::semantic::Semantic;
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{
        binding::NativeResult, context::CallContext, scalar::NativeScalar, types::VariantRef,
    },
    value::Value,
};
use kagari_types::language::binding;
use std::{ops::Bound, sync::OnceLock};

pub(crate) type Entry = for<'call> fn(&mut CallContext<'call>) -> NativeResult<Value>;
static OPTION_MEMBERS: OnceLock<NativeResult<(VariantRef, VariantRef)>> = OnceLock::new();

pub(crate) fn initialize(semantic: &Semantic) -> NativeResult<()> {
    OPTION_MEMBERS
        .get_or_init(|| {
            let option = binding::option_declaration();
            Ok((
                semantic.variant(&option, "Some")?.clone(),
                semantic.variant(&option, "None")?.clone(),
            ))
        })
        .as_ref()
        .map(|_| ())
        .map_err(Clone::clone)
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

pub(crate) fn index_argument(cx: &CallContext<'_>, slot: usize) -> NativeResult<u64> {
    u64::decode(cx.argument(slot)?)
}

pub(crate) fn list_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.allocate_result()
}

/// Only immutable declaration handles are shared. Each allocation still resolves
/// and checks the result's runtime-local, generation-pinned enum layout.
pub(crate) fn option(cx: &CallContext<'_>, value: Option<Value>) -> NativeResult<Value> {
    let (some, none) = option_members()?;
    cx.allocate_enum(
        &cx.result_type_argument()?,
        if value.is_some() { some } else { none },
        value.into_iter().collect(),
    )
}

fn option_members() -> NativeResult<&'static (VariantRef, VariantRef)> {
    OPTION_MEMBERS
        .get()
        .ok_or_else(|| RuntimeError::metadata_conflict("foundation Option members are not linked"))?
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn list_pop(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    cx.ensure_collection_mutable(id)?;
    let length = cx.heap().sequence_len(id).ok_or_else(invalid)?;
    let value = length
        .checked_sub(1)
        .and_then(|index| cx.heap().sequence_get(id, index));
    // Allocate the result before committing the write. No callback or safepoint
    // can invalidate this preparation before the removal.
    let result = option(cx, value)?;
    cx.heap().sequence_pop(id)?;
    Ok(result)
}

pub(crate) fn list_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    let index = usize::try_from(index_argument(cx, 1)?).ok();
    cx.ensure_collection_mutable(id)?;
    let result = option(
        cx,
        index.and_then(|index| cx.heap().sequence_get(id, index)),
    )?;
    if let Some(index) = index {
        cx.heap().sequence_remove(id, index)?;
    }
    Ok(result)
}

pub(crate) fn iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.iter_operation(0, IterOp::New)
}

pub(crate) fn next(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let (some, none) = option_members()?;
    cx.iterator_next_result(0, some, none)
}

pub(crate) fn start_bound(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(cx, false)
}

pub(crate) fn end_bound(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    range_bound(cx, true)
}

fn range_bound(cx: &mut CallContext<'_>, upper: bool) -> NativeResult<Value> {
    let Value::Range(range) = cx.argument(0)? else {
        return Err(invalid());
    };
    let range = cx.heap().range(range).ok_or_else(invalid)?;
    let (member, fields) = match range.bound(
        cx.owner().definitions(),
        cx.argument_type(0)?,
        cx.result_type(),
        upper,
    )? {
        Bound::Included(value) => ("Included", vec![value]),
        Bound::Excluded(value) => ("Excluded", vec![value]),
        Bound::Unbounded => ("Unbounded", vec![]),
    };
    enums::allocate(cx, &cx.result_type_argument()?, "Bound", member, fields)
}
