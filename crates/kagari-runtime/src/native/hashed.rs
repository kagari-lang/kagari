//! Shared traced Rust hash-table payloads. Script hashing never borrows a table.
use crate::{
    error::RuntimeError,
    native::{
        binding::NativeResult,
        hash_storage::{HashMapStorage, HashSetStorage},
        storage::{NativePayload, NativeStorage, StorageContext},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_abi::{
    standard::RuntimePrimitive,
    types::{AbiType, native::NativeStorageLayout},
};
use std::rc::Rc;

#[derive(Debug)]
pub(crate) struct MapPayload {
    pub(crate) key: Rc<StorageType>,
    pub(crate) value: Rc<StorageType>,
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
    fn units(&self) -> usize {
        self.entries.len()
    }
}
#[derive(Debug)]
pub(crate) struct SetPayload {
    pub(crate) element: Rc<StorageType>,
    pub(crate) builtin_keys: bool,
    pub(crate) entries: HashSetStorage,
}
impl NativePayload for SetPayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for key in self.entries.iter() {
            visit(key.value());
        }
    }
    fn units(&self) -> usize {
        self.entries.len()
    }
}
fn builtin_keys(context: &StorageContext<'_>, key: &AbiType) -> NativeResult<bool> {
    let hash = context.selected(0)?;
    let equal = context.selected(1)?;
    if hash.params.as_ref() != [key.clone()] || equal.params.as_ref() != [key.clone(), key.clone()]
    {
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
            let (key, value) = match context.ty() {
                AbiType::Map { key, value, .. } => (key.as_ref(), value.as_ref()),
                AbiType::NativeObject(nominal) => (
                    nominal.arguments.get(key).ok_or_else(invalid)?,
                    nominal.arguments.get(value).ok_or_else(invalid)?,
                ),
                _ => return Err(invalid()),
            };
            Ok(MapPayload {
                key: Rc::new(StorageType::prepare(key.clone(), context.owner())?),
                value: Rc::new(StorageType::prepare(value.clone(), context.owner())?),
                builtin_keys: builtin_keys(context, key)?,
                entries: HashMapStorage::new(),
            })
        })
    }
    pub(crate) fn set(element: usize) -> Self {
        Self::with_layout(NativeStorageLayout::Set { element }, move |context| {
            let element = match context.ty() {
                AbiType::Set(element, _) => element.as_ref(),
                AbiType::NativeObject(nominal) => {
                    nominal.arguments.get(element).ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            };
            Ok(SetPayload {
                element: Rc::new(StorageType::prepare(element.clone(), context.owner())?),
                builtin_keys: builtin_keys(context, element)?,
                entries: HashSetStorage::new(),
            })
        })
    }
}
fn invalid() -> RuntimeError {
    RuntimeError::module_validation("hash storage type arguments")
}
