//! Shared algorithms and the temporary preparation bridge for SR04 Future/Task owners.
//! SR05 removes this routing owner; migrated declarations never enter it.
pub(crate) mod arrays;
pub(crate) mod comparison;
pub(crate) mod construction;
mod enums;
pub(crate) mod hash;
pub(crate) mod iterable;
pub(crate) mod lists;
pub(crate) mod propagation;
mod tasks;
pub(crate) mod vectors;
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
            NativeStorageLayout::Future => NativeStorage::future(),
            NativeStorageLayout::Task => NativeStorage::task(),
            NativeStorageLayout::TaskScope => NativeStorage::task_scope(),
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
    let entry: Entry = match name {
        "$foundation_task_spawn" => tasks::spawn,
        "$foundation_task_cancel" => tasks::cancel,
        _ => {
            return Err(RuntimeError::metadata_conflict(format!(
                "missing foundation implementation {name}"
            )));
        }
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
