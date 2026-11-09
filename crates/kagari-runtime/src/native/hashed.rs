//! Shared traced Rust hash-table payloads. Script hashing never borrows a table.
use crate::{
    error::RuntimeError,
    frame::types::compatibility::TypeView,
    native::{
        binding::NativeResult,
        context::LinkedCallable,
        hash_storage::{HashMapStorage, HashSetStorage},
        storage::{NativePayload, NativeStorage, StorageContext},
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
};
use kagari_contract::standard::RuntimePrimitive;
use kagari_types::declaration::native::NativeStorageLayout;
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct MapPayload {
    pub(crate) key: Arc<StorageType>,
    pub(crate) value: Arc<StorageType>,
    pub(crate) builtin_keys: bool,
    pub(crate) entries: HashMapStorage,
}

impl NativePayload for MapPayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for (key, value) in self.entries.iter() {
            visit(key.value());
            visit(value);
        }
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(
            self.entries.len(),
            self.entries.pressure_bytes(),
        ))
    }
}

#[derive(Debug)]
pub(crate) struct SetPayload {
    pub(crate) element: Arc<StorageType>,
    pub(crate) builtin_keys: bool,
    pub(crate) entries: HashSetStorage,
}

impl NativePayload for SetPayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for key in self.entries.iter() {
            visit(key.value());
        }
    }

    fn charge(&self) -> NativeResult<HeapCharge> {
        Ok(HeapCharge::new(
            self.entries.len(),
            self.entries.pressure_bytes(),
        ))
    }
}

fn matches_key(
    context: &StorageContext<'_>,
    key: &StorageType,
    callable: &LinkedCallable,
    count: usize,
) -> NativeResult<bool> {
    let owner = callable.owner(context.owner())?;
    if let Some(signature) = &callable.scoped_signature {
        return Ok(signature.params.len() == count
            && signature
                .params
                .iter()
                .all(|argument| key.matches_view(argument.view(&owner))));
    }
    Ok(callable.params.len() == count
        && callable.params.iter().all(|ty| {
            key.matches_view(TypeView::new(
                ty,
                &owner,
                callable
                    .environment
                    .as_ref()
                    .map(|environment| environment.types.as_ref()),
            ))
        }))
}

fn builtin_keys(context: &StorageContext<'_>, key: &StorageType) -> NativeResult<bool> {
    let hash = context.selected(0)?;
    let equal = context.selected(1)?;
    if !matches_key(context, key, hash, 1)? || !matches_key(context, key, equal, 2)? {
        return Err(RuntimeError::module_validation(
            "hash storage callable receiver differs from its key type",
        ));
    }
    Ok(hash.primitive == Some(RuntimePrimitive::ValueHash)
        && equal.primitive == Some(RuntimePrimitive::ValueEq))
}

impl NativeStorage {
    pub(crate) fn map(key: usize, value: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Map { key, value }, move |context| {
            let key_contract = context.element_contract(key)?;
            let value_contract = context.element_contract(value)?;
            let builtin_keys = builtin_keys(context, &key_contract)?;
            Ok(MapPayload {
                key: key_contract,
                value: value_contract,
                builtin_keys,
                entries: HashMapStorage::new(),
            })
        })
    }

    pub(crate) fn set(element: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Set { element }, move |context| {
            let contract = context.element_contract(element)?;
            let builtin_keys = builtin_keys(context, &contract)?;
            Ok(SetPayload {
                element: contract,
                builtin_keys,
                entries: HashSetStorage::new(),
            })
        })
    }
}
