//! Mandatory Rust implementations of the compiler-owned language foundation.
//! The declaration catalog is the single authority for every signature and bound.
mod construction;
mod enums;
mod hash;
mod lists;
mod propagation;
mod strings;
mod vectors;
use crate::{catalog, collections, declarations::StandardDeclarations, namespaces};
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{
        binding::{Codec, NativeBinding, NativeResult},
        builder::ModuleBuilder,
        context::CallContext,
        module::NativeModule,
        scalar::NativeScalar,
        storage::NativeStorage,
    },
    value::Value,
};
use kagari_types::{callable::CallableImplementation, declaration::module::ModuleDecl};
use std::{collections::BTreeMap, ops::Bound, sync::OnceLock};

type Entry = for<'call> fn(&mut CallContext<'call>) -> NativeResult<Value>;

// Shared registrations contain immutable declarations and Send + Sync callbacks.
static MODULE: OnceLock<NativeResult<Vec<NativeModule>>> = OnceLock::new();

pub fn modules() -> NativeResult<Vec<NativeModule>> {
    MODULE.get_or_init(build).clone()
}

fn build() -> NativeResult<Vec<NativeModule>> {
    let language = StandardDeclarations::default();
    catalog::declarations()
        .into_iter()
        .map(|declaration| build_module(declaration, &language))
        .collect()
}

fn build_module(
    declaration: ModuleDecl,
    language: &StandardDeclarations,
) -> NativeResult<NativeModule> {
    let catalog = language.catalog()?;
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
        if let Some(binding) = strings::binding(name, &catalog)? {
            bindings.insert(id.clone(), binding);
            continue;
        }
        if let Some(binding) = vectors::binding(name, &function)? {
            bindings.insert(id.clone(), binding);
            continue;
        }
        let entry: Entry = match name {
            "$foundation_propagation_Option_branch" => propagation::option_branch,
            "$foundation_propagation_Result_branch" => propagation::result_branch,
            "$foundation_propagation_ControlFlow_branch" => propagation::control_flow_branch,
            "$foundation_propagation_Option_from_output" => propagation::option_from_output,
            "$foundation_propagation_Result_from_output" => propagation::result_from_output,
            "$foundation_propagation_ControlFlow_from_output" => {
                propagation::control_flow_from_output
            }
            "$foundation_propagation_Option_from_residual" => propagation::option_from_residual,
            "$foundation_propagation_Result_from_residual" => propagation::result_from_residual,
            "$foundation_propagation_ControlFlow_from_residual" => {
                propagation::control_flow_from_residual
            }
            "$foundation_try_from" => construction::try_from,
            "$foundation_from_str" => construction::from_str,
            "$foundation_sum" => construction::sum,
            "$foundation_product" => construction::product,
            "$foundation_list_from_iter" => construction::list_from_iter,
            "$foundation_list_new" => list_new,
            "$foundation_list_pop" => list_pop,
            "$foundation_list_remove" => list_remove,
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
            _ => lists::entry(name).ok_or_else(|| {
                RuntimeError::metadata_conflict(format!("missing foundation implementation {name}"))
            })?,
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
    let collections_module = declaration.identity == namespaces::module("std", "collections");
    let future_module = declaration.identity == namespaces::module("core", "future");
    let mut builder = ModuleBuilder::from_declaration(declaration, &catalog, bindings);
    if future_module {
        builder.bind_storage(&catalog.future_type()?, NativeStorage::future())?;
    }
    if collections_module {
        collections::register(&mut builder, language)?;
    }
    builder.finish()
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

pub(super) fn option(cx: &CallContext<'_>, value: Option<Value>) -> NativeResult<Value> {
    enums::allocate(
        cx,
        &cx.result_type_argument()?,
        "Option",
        if value.is_some() { "Some" } else { "None" },
        value.into_iter().collect(),
    )
}

fn list_pop(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    cx.ensure_collection_mutable(id)?;
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

fn list_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let id = array(cx)?;
    let index = index(cx, 1)?;
    cx.ensure_collection_mutable(id)?;
    let result = option(cx, cx.heap().array_get(id, index))?;
    cx.heap().array_remove(id, index)?;
    Ok(result)
}

fn iter(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.iter_operation(0, IterOp::New)
}

fn next(cx: &mut CallContext<'_>) -> NativeResult<Value> {
    cx.iter_operation(0, IterOp::Next)
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
