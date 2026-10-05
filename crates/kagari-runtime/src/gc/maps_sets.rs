use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::{
        hash_storage::{HashMapStorage, HashSetStorage},
        hashed::{MapPayload, SetPayload},
        storage_type::StorageType,
    },
    value::{MapKey, Value},
};
use kagari_types::ty::Ty;
use std::sync::Arc;

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "hash storage type or selected key protocol mismatch",
    )
}

impl GcHeap {
    pub fn map_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_map(id, |entries| entries.len())
    }

    pub fn map_snapshot(&self, id: HeapObjectId) -> Option<Vec<(Value, Value)>> {
        self.with_map(id, |entries| {
            entries
                .iter()
                .map(|(key, value)| (key.to_value(), value.clone()))
                .collect()
        })
    }

    pub fn map_get(&self, id: HeapObjectId, key: &Value) -> Option<Value> {
        let (contract, _, builtin) = self.map_contract(id)?;
        if !builtin || !self.valid_storage_value(key, &contract) {
            return None;
        }
        let key = MapKey::from_value(self, key)?;
        self.with_map(id, |entries| entries.get(&key).cloned())
            .flatten()
    }

    pub fn map_insert(
        &self,
        id: HeapObjectId,
        key: Value,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let (key_contract, value_contract, builtin) = self.map_contract(id).ok_or_else(invalid)?;
        if !builtin
            || !self.valid_storage_value(&key, &key_contract)
            || !self.valid_storage_value(&value, &value_contract)
        {
            return Err(invalid());
        }
        let key = MapKey::from_value(self, &key)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.ensure_key_mutable(id)?;
        self.with_map_mut(id, |entries| {
            let units = usize::from(!entries.contains_key(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            let growth = self.resources.prepare_heap_growth(units)?;
            entries
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            entries
                .insert(key, value)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn map_remove(&self, id: HeapObjectId, key: &Value) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let (contract, _, builtin) = self.map_contract(id).ok_or_else(invalid)?;
        if !builtin || !self.valid_storage_value(key, &contract) {
            return Err(invalid());
        }
        let key = MapKey::from_value(self, key)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        let value = self
            .with_map_mut(id, |entries| entries.remove(&key))
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn map_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_map_mut(id, |entries| {
                let removed = entries.len();
                entries.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn set_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_set(id, |values| values.len())
    }

    pub fn set_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.with_set(id, |values| values.iter().map(MapKey::to_value).collect())
    }

    pub fn set_contains(&self, id: HeapObjectId, value: &Value) -> Option<bool> {
        let (contract, builtin) = self.set_contract(id)?;
        if !builtin || !self.valid_storage_value(value, &contract) {
            return None;
        }
        let key = MapKey::from_value(self, value)?;
        self.with_set(id, |values| values.contains(&key))
    }

    pub fn set_insert(&self, id: HeapObjectId, value: Value) -> Result<bool, RuntimeError> {
        self.ensure_execution_allowed()?;
        let (contract, builtin) = self.set_contract(id).ok_or_else(invalid)?;
        if !builtin || !self.valid_storage_value(&value, &contract) {
            return Err(invalid());
        }
        let key = MapKey::from_value(self, &value)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.ensure_key_mutable(id)?;
        self.with_set_mut(id, |values| {
            let units = usize::from(!values.contains(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            let growth = self.resources.prepare_heap_growth(units)?;
            values
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            let inserted = values
                .insert(key)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            growth.commit();
            Ok(inserted)
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn set_remove(&self, id: HeapObjectId, value: &Value) -> Result<bool, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let (contract, builtin) = self.set_contract(id).ok_or_else(invalid)?;
        if !builtin || !self.valid_storage_value(value, &contract) {
            return Err(invalid());
        }
        let key = MapKey::from_value(self, value)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        let removed = self
            .with_set_mut(id, |values| values.remove(&key))
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if removed {
            self.release_heap_units(1);
        }
        Ok(removed)
    }

    pub fn set_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_set_mut(id, |values| {
                let removed = values.len();
                values.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub(crate) fn map_contract(
        &self,
        id: HeapObjectId,
    ) -> Option<(Arc<StorageType>, Arc<StorageType>, bool)> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Map { .. }) {
            return None;
        }
        let payload = object.payload::<MapPayload>().ok()?;
        Some((
            payload.key.clone(),
            payload.value.clone(),
            payload.builtin_keys,
        ))
    }

    pub(crate) fn set_contract(&self, id: HeapObjectId) -> Option<(Arc<StorageType>, bool)> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Set(..)) {
            return None;
        }
        let payload = object.payload::<SetPayload>().ok()?;
        Some((payload.element.clone(), payload.builtin_keys))
    }

    pub(super) fn valid_storage_value(&self, value: &Value, contract: &StorageType) -> bool {
        self.valid_payload(value) && contract.accepts_value(self, value)
    }

    pub(super) fn with_map<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&HashMapStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Map { .. }) {
            return None;
        }
        Some(f(&object.payload::<MapPayload>().ok()?.entries))
    }

    pub(super) fn with_map_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut HashMapStorage) -> R,
    ) -> Option<R> {
        let mut objects = self.objects_mut().ok()?;
        let revision = objects.get(id.slot)?.revision.checked_add(1)?;
        let HeapObject::Native(object) = self.object_mut(&mut objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Map { .. }) {
            return None;
        }
        let entries = &mut object.payload_mut::<MapPayload>().ok()?.entries;
        let old_len = entries.len();
        let result = f(entries);
        if entries.len() != old_len {
            objects[id.slot].revision = revision;
        }
        Some(result)
    }

    pub(super) fn with_set<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&HashSetStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Set(..)) {
            return None;
        }
        Some(f(&object.payload::<SetPayload>().ok()?.entries))
    }

    pub(super) fn with_set_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut HashSetStorage) -> R,
    ) -> Option<R> {
        let mut objects = self.objects_mut().ok()?;
        let revision = objects.get(id.slot)?.revision.checked_add(1)?;
        let HeapObject::Native(object) = self.object_mut(&mut objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Set(..)) {
            return None;
        }
        let entries = &mut object.payload_mut::<SetPayload>().ok()?.entries;
        let old_len = entries.len();
        let result = f(entries);
        if entries.len() != old_len {
            objects[id.slot].revision = revision;
        }
        Some(result)
    }
}
