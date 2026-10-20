use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId},
    native::{
        hash_storage::{HashMapStorage, HashSetStorage},
        hashed::{MapPayload, SetPayload},
        storage_type::StorageType,
    },
    resource::HeapCharge,
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
                .map(|(key, value)| (key.to_value(), *value))
                .collect()
        })
    }

    pub fn map_get(&self, id: HeapObjectId, key: &Value) -> Option<Value> {
        self.with_map_payload(id, |payload| {
            if !payload.builtin_keys || !self.valid_storage_value(key, &payload.key) {
                return None;
            }
            let key = MapKey::from_value(self, key)?;
            payload.entries.get(&key).copied()
        })?
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
        self.with_map_mut(id, 1, |entries| {
            let units = usize::from(!entries.contains_key(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            entries
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            entries
                .insert(key, value)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            Ok(())
        })?
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
        let value = self.with_map_mut(id, 0, |entries| entries.remove(&key))?;
        Ok(value)
    }

    pub fn map_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let _removed = self.with_map_mut(id, 0, |entries| {
            let removed = entries.len();
            entries.clear();
            removed
        })?;
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
        self.with_set_mut(id, 1, |values| {
            let units = usize::from(!values.contains(&key));
            if units != 0 {
                self.ensure_structure_mutable(id)?;
            }
            values
                .try_reserve(units)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            let inserted = values
                .insert(key)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            Ok(inserted)
        })?
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
        let removed = self.with_set_mut(id, 0, |values| values.remove(&key))?;
        Ok(removed)
    }

    pub fn set_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let _removed = self.with_set_mut(id, 0, |values| {
            let removed = values.len();
            values.clear();
            removed
        })?;
        Ok(())
    }

    pub(crate) fn map_contract(
        &self,
        id: HeapObjectId,
    ) -> Option<(Arc<StorageType>, Arc<StorageType>, bool)> {
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
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
        let object = self.native_ref(&objects, id)?;
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
        self.with_map_payload(id, |payload| f(&payload.entries))
    }

    /// Builtin lookup borrows prepared contracts and entries together. All key
    /// inspection is read-only; user Hash/Eq callbacks use custom_keys instead.
    fn with_map_payload<R>(&self, id: HeapObjectId, f: impl FnOnce(&MapPayload) -> R) -> Option<R> {
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
        if !matches!(object.ty, Ty::Map { .. }) {
            return None;
        }
        Some(f(object.payload::<MapPayload>().ok()?))
    }

    pub(super) fn with_map_mut<R>(
        &self,
        id: HeapObjectId,
        growth: usize,
        f: impl FnOnce(&mut HashMapStorage) -> R,
    ) -> Result<R, RuntimeError> {
        let invalid = || RuntimeError::module_validation("invalid hash storage");
        let mut objects = self.objects_mut()?;
        let object = self.native_mut(&mut objects, id).ok_or_else(invalid)?;
        let revision = object.revision.checked_add(1).ok_or_else(invalid)?;
        if !matches!(object.ty, Ty::Map { .. }) {
            return Err(invalid());
        }
        let max_units = object
            .charge
            .logical_units
            .checked_add(growth)
            .ok_or_else(|| self.resource_limit("hash length"))?;
        let change = self
            .resources
            .prepare_heap_change(object.charge, max_units)?;
        let entries = &mut object.payload_mut::<MapPayload>()?.entries;
        let old_len = entries.len();
        let result = f(entries);
        let changed = entries.len() != old_len;
        let charge = HeapCharge::new(entries.len(), entries.pressure_bytes());
        let charge = object.storage_charge(charge);
        if changed {
            object.revision = revision;
        }
        object.charge = charge;
        change.commit(charge);
        Ok(result)
    }

    pub(super) fn with_set<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&HashSetStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
        if !matches!(object.ty, Ty::Set(..)) {
            return None;
        }
        Some(f(&object.payload::<SetPayload>().ok()?.entries))
    }

    pub(super) fn with_set_mut<R>(
        &self,
        id: HeapObjectId,
        growth: usize,
        f: impl FnOnce(&mut HashSetStorage) -> R,
    ) -> Result<R, RuntimeError> {
        let invalid = || RuntimeError::module_validation("invalid hash storage");
        let mut objects = self.objects_mut()?;
        let object = self.native_mut(&mut objects, id).ok_or_else(invalid)?;
        let revision = object.revision.checked_add(1).ok_or_else(invalid)?;
        if !matches!(object.ty, Ty::Set(..)) {
            return Err(invalid());
        }
        let max_units = object
            .charge
            .logical_units
            .checked_add(growth)
            .ok_or_else(|| self.resource_limit("hash length"))?;
        let change = self
            .resources
            .prepare_heap_change(object.charge, max_units)?;
        let entries = &mut object.payload_mut::<SetPayload>()?.entries;
        let old_len = entries.len();
        let result = f(entries);
        let changed = entries.len() != old_len;
        let charge = HeapCharge::new(entries.len(), entries.pressure_bytes());
        let charge = object.storage_charge(charge);
        if changed {
            object.revision = revision;
        }
        object.charge = charge;
        change.commit(charge);
        Ok(result)
    }
}
