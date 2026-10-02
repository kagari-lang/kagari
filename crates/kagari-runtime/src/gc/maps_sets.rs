use crate::gc::hash_storage::{HashMapStorage, HashSetStorage};
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObject, HeapObjectId},
    value::{MapKey, Value},
};

impl GcHeap {
    pub fn alloc_map(&self, entries: Vec<(Value, Value)>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut map = HashMapStorage::new();
        for (key, value) in entries {
            if !self.valid_payload(&value) {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid heap target, index, or payload",
                ));
            }
            let key = MapKey::from_value(self, &key).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key")
            })?;
            map.try_reserve(usize::from(!map.contains_key(&key)))
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            map.insert(key, value)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
        }
        self.alloc_object(HeapObject::Map(map))
    }

    pub fn alloc_set(&self, values: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let mut set = HashSetStorage::new();
        for value in values {
            let key = MapKey::from_value(self, &value).ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key")
            })?;
            set.try_reserve(usize::from(!set.contains(&key)))
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            set.insert(key)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
        }
        self.alloc_object(HeapObject::Set(set))
    }

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
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        let key = MapKey::from_value(self, &key)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.ensure_key_mutable(id)?;
        self.with_map_mut(id, |entries| {
            if entries
                .iter()
                .next()
                .is_some_and(|(key, _)| key.custom_parts().is_some())
            {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "custom keys require script protocol execution",
                ));
            }
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
        let key = MapKey::from_value(self, value)?;
        self.with_set(id, |values| values.contains(&key))
    }

    pub fn set_insert(&self, id: HeapObjectId, value: Value) -> Result<bool, RuntimeError> {
        self.ensure_execution_allowed()?;
        let key = MapKey::from_value(self, &value)
            .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid hash key"))?;
        self.ensure_key_mutable(id)?;
        self.with_set_mut(id, |values| {
            if values
                .iter()
                .next()
                .is_some_and(|key| key.custom_parts().is_some())
            {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "custom keys require script protocol execution",
                ));
            }
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

    pub(super) fn with_map<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&HashMapStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Map(entries) => Some(f(entries)),
            HeapObject::Array(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(..)
            | HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::ManagedIter(_) => None,
        }
    }

    pub(super) fn with_map_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut HashMapStorage) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        let revision = objects.get(id.slot)?.revision.checked_add(1)?;
        match self.object_mut(&mut objects, id)? {
            HeapObject::Map(entries) => {
                let old_len = entries.len();
                let result = f(entries);
                if entries.len() != old_len {
                    objects[id.slot].revision = revision;
                }
                Some(result)
            }
            HeapObject::Array(_)
            | HeapObject::Set(_)
            | HeapObject::Enum(..)
            | HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::ManagedIter(_) => None,
        }
    }

    pub(super) fn with_set<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&HashSetStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Set(values) => Some(f(values)),
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Enum(..)
            | HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::ManagedIter(_) => None,
        }
    }

    pub(super) fn with_set_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut HashSetStorage) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        let revision = objects.get(id.slot)?.revision.checked_add(1)?;
        match self.object_mut(&mut objects, id)? {
            HeapObject::Set(values) => {
                let old_len = values.len();
                let result = f(values);
                if values.len() != old_len {
                    objects[id.slot].revision = revision;
                }
                Some(result)
            }
            HeapObject::Array(_)
            | HeapObject::Map(_)
            | HeapObject::Enum(..)
            | HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::ManagedIter(_) => None,
        }
    }
}
