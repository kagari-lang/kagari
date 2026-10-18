//! Temporary direct preparation bridge for remaining SR03/SR04 contributors.
//! SR05 removes this routing owner; migrated declarations never enter it.
pub(crate) mod arrays;
mod comparison;
pub(crate) mod construction;
mod enums;
mod hash;
mod iterable;
mod lists;
pub(crate) mod propagation;
mod tasks;
mod vectors;
use crate::{assembly::semantic::Semantic, identity};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        DefinitionKind,
        metadata::DefinitionMetadata,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_contract::operations::IterOp;
use kagari_runtime::{
    error::RuntimeError,
    gc::HeapObjectId,
    native::{
        assembly::{
            authoring::{PendingBinding, SealedPreparation},
            draft::ModuleDraft,
        },
        binding::{Codec, NativeBinding, NativeResult},
        context::CallContext,
        scalar::NativeScalar,
        storage::NativeStorage,
        types::VariantRef,
    },
    value::Value,
};
use kagari_types::{
    callable::CallableImplementation,
    declaration::{TypeDefKind, module::ModuleDecl, native::NativeStorageLayout},
    language::binding,
};
use std::{collections::BTreeMap, ops::Bound, sync::OnceLock};

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

pub(crate) fn collect(
    module: &mut ModuleDraft<'_>,
    source: DefinitionMetadata<ModuleDecl>,
) -> NativeResult<()> {
    let definitions = source.definitions();
    let mut declaration = source.records().clone();
    let mut bindings = BTreeMap::new();
    for function in &mut declaration.functions {
        let id = source
            .records()
            .definition(DefinitionKind::Function, &function.name, definitions)
            .map_err(conflict)?;
        associate(&mut function.implementation, id, definitions, &mut bindings)?;
    }
    for (index, implementation) in declaration.implementations.iter_mut().enumerate() {
        let owner = source
            .records()
            .implementation_id(index, definitions)
            .map_err(conflict)?;
        for method in &mut implementation.methods {
            let id = ModuleDecl::method_id(owner, &method.name, definitions).map_err(conflict)?;
            associate(&mut method.implementation, id, definitions, &mut bindings)?;
        }
    }
    let source = DefinitionMetadata::checked(
        definitions.clone(),
        declaration,
        &CancellationToken::default(),
    )
    .map_err(conflict)?;
    module.import_declaration(&source, bindings)?;
    for ty in &source.records().types {
        let TypeDefKind::NativeStorage(layout) = ty.kind else {
            continue;
        };
        let reference = module.reference_type(
            identity::type_id(&ty.name, ty.kind.definition_kind()),
            ty.kind,
            ty.generic_params.len(),
        )?;
        let storage = match layout {
            NativeStorageLayout::Sequence { element } => NativeStorage::sequence(element),
            NativeStorageLayout::Future => NativeStorage::future(),
            NativeStorageLayout::Task => NativeStorage::task(),
            NativeStorageLayout::TaskScope => NativeStorage::task_scope(),
            NativeStorageLayout::Opaque
                if matches!(
                    ty.name.as_str(),
                    "MapIterator"
                        | "FilterIterator"
                        | "TakeIterator"
                        | "FilterMapIterator"
                        | "SkipIterator"
                        | "EnumerateIterator"
                        | "ZipIterator"
                        | "ChainIterator"
                ) =>
            {
                iterable::storage()
            }
            _ => return Err(conflict("unexpected remaining storage contributor")),
        };
        module.bind_storage(&reference, storage)?;
    }
    Ok(())
}

fn associate(
    implementation: &mut CallableImplementation,
    declaration: DefinitionId,
    definitions: &DefinitionTable,
    bindings: &mut BTreeMap<DefinitionId, PendingBinding>,
) -> NativeResult<()> {
    let CallableImplementation::Native(target) = implementation else {
        return Err(conflict("remaining body is not a native association"));
    };
    let name = definitions
        .resolve(*target)
        .map_err(conflict)?
        .last()
        .ok_or_else(|| conflict("missing native target"))?
        .name
        .to_owned();
    // Legacy shared target spellings select only the remaining algorithm. Every
    // submitted declaration still owns an independent checked native association.
    *target = declaration;
    if bindings
        .insert(
            declaration,
            PendingBinding::implementation_with_send(move |preparation| {
                prepare_remaining(&name, preparation)
            }),
        )
        .is_some()
    {
        return Err(conflict("duplicate remaining native association"));
    }
    Ok(())
}

fn prepare_remaining(
    name: &str,
    preparation: &SealedPreparation<'_>,
) -> NativeResult<NativeBinding> {
    if let Some(binding) = comparison::binding(name, preparation.declaration())? {
        return Ok(binding);
    }
    if name == "$foundation_default_Vec" {
        return Ok(NativeBinding::new(vec![], Codec::Value, |cx| {
            cx.allocate_result()
        }));
    }
    if let Some(binding) = vectors::binding(name, preparation.declaration())? {
        return Ok(binding);
    }
    if let Some(binding) = iterable::binding(name) {
        return Ok(binding);
    }
    let entry: Entry = match name {
        "$foundation_task_spawn" => tasks::spawn,
        "$foundation_task_cancel" => tasks::cancel,
        "$foundation_map_from_iter" | "$foundation_set_from_iter" => construction::hash_from_iter,
        "$foundation_list_from_iter" => construction::list_from_iter,
        "$foundation_list_from" => arrays::vector,
        "$foundation_set_from" => arrays::set,
        "$foundation_list_new" => list_new,
        "$foundation_list_pop" => list_pop,
        "$foundation_list_remove" => list_remove,
        "$foundation_list_iter" | "$foundation_map_iter" | "$foundation_set_iter" => iter,
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
        _ => lists::entry(name).ok_or_else(|| {
            RuntimeError::metadata_conflict(format!("missing foundation implementation {name}"))
        })?,
    };
    Ok(NativeBinding::new(
        vec![Codec::Value; preparation.declaration().records().function.params.len()],
        Codec::Value,
        entry,
    ))
}

fn conflict(error: impl ToString) -> RuntimeError {
    RuntimeError::metadata_conflict(error.to_string())
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

fn list_new(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

fn list_pop(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

fn list_remove(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

fn next(cx: &mut CallContext<'_>) -> NativeResult<Value> {
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
