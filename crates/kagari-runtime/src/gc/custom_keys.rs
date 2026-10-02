//! Hash bucket preparation and atomic commits. Script comparisons run in VM
//! frames between these operations, never inside a borrowed hash table.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{CollectionIteration, GcHeap, GcObjectKind, HeapObjectId},
    value::{MapKey, Value},
};

use crate::gc::hash_storage::HashMapStorage;

fn invalid() -> RuntimeError {
    RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid custom key operation")
}

impl GcHeap {
    /// Empty collections have no stored key representation yet. Once populated,
    /// raw builtin lookup and compiled custom lookup must not be mixed.
    pub fn ensure_key_mode(&self, collection: &Value, custom: bool) -> Result<(), RuntimeError> {
        let mode = match collection {
            Value::Map(id) => self.with_map(*id, |entries| {
                entries
                    .iter()
                    .next()
                    .map(|(key, _)| key.custom_parts().is_some())
            }),
            Value::Set(id) => self.with_set(*id, |entries| {
                entries
                    .iter()
                    .next()
                    .map(|key| key.custom_parts().is_some())
            }),
            _ => None,
        }
        .ok_or_else(invalid)?;
        if mode.is_some_and(|mode| mode != custom) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "collection key protocol mismatch; custom keys require script protocol execution",
            ));
        }
        Ok(())
    }

    pub(crate) fn begin_key_lookup(
        &self,
        value: &Value,
    ) -> Result<CollectionIteration, RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match value {
            Value::Map(id) if self.object_kind(*id) == Some(GcObjectKind::Map) => *id,
            Value::Set(id) if self.object_kind(*id) == Some(GcObjectKind::Set) => *id,
            _ => return Err(invalid()),
        };
        let root = self.root_value(value.clone()).ok_or_else(invalid)?;
        let mut active = self.key_lookups.borrow_mut();
        let count = active
            .get(&id)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or_else(invalid)?;
        active
            .try_reserve(1)
            .map_err(|_| self.resource_limit("key lookup registry"))?;
        active.insert(id, count);
        Ok(CollectionIteration {
            _children: Vec::new(),
            iter_loops: Vec::new(),
            active: self.key_lookups.clone(),
            id: Some(id),
            _root: root,
        })
    }

    pub(crate) fn ensure_key_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_callback_mutable(id)?;
        if self.key_lookups.borrow().contains_key(&id) {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "container mutation during key comparison or hashing",
            ))
        } else {
            Ok(())
        }
    }

    pub fn custom_candidates(
        &self,
        collection: &Value,
        hash: i64,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.ensure_key_mode(collection, true)?;
        match collection {
            Value::Map(id) => self.with_map(*id, |entries| Ok(entries.candidates(hash))),
            Value::Set(id) => self.with_set(*id, |entries| Ok(entries.candidates(hash))),
            _ => None,
        }
        .ok_or_else(invalid)?
    }

    pub fn custom_get(
        &self,
        collection: &Value,
        hash: i64,
        token: i64,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_key_mode(collection, true)?;
        let key = MapKey::custom(hash, token, Value::Unit);
        match collection {
            Value::Map(id) => self.with_map(*id, |entries| entries.get(&key).cloned()),
            Value::Set(id) => self.with_set(*id, |entries| entries.get(&key).map(MapKey::to_value)),
            _ => None,
        }
        .ok_or_else(invalid)
    }

    pub fn custom_insert(
        &self,
        collection: &Value,
        hash: i64,
        token: i64,
        key: Value,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&key) || !self.valid_payload(&value) {
            return Err(invalid());
        }
        let id = match collection {
            Value::Map(id) | Value::Set(id) => *id,
            _ => return Err(invalid()),
        };
        self.ensure_key_mutable(id)?;
        self.ensure_key_mode(collection, true)?;
        let new = token == -1;
        let token = if new {
            self.ensure_structure_mutable(id)?;
            let token = self.next_key_token.get();
            self.next_key_token
                .set(token.checked_add(1).ok_or_else(invalid)?);
            token
        } else {
            token
        };
        let growth = self.resources.prepare_heap_growth(usize::from(new))?;
        let key = MapKey::custom(hash, token, key);
        let insert = |entries: &mut HashMapStorage| -> Result<(), RuntimeError> {
            if new {
                entries
                    .try_reserve(1)
                    .map_err(|_| self.resource_limit("allocation capacity"))?;
                entries
                    .insert(key.clone(), value.clone())
                    .map_err(|_| self.resource_limit("allocation capacity"))?;
            } else {
                *entries.get_mut(&key).ok_or_else(invalid)? = value.clone();
            }
            Ok(())
        };
        match collection {
            Value::Map(_) => self.with_map_mut(id, insert).ok_or_else(invalid)??,
            Value::Set(_) => self
                .with_set_mut(id, |entries| {
                    if new {
                        entries
                            .try_reserve(1)
                            .map_err(|_| self.resource_limit("allocation capacity"))?;
                        entries
                            .insert(key)
                            .map_err(|_| self.resource_limit("allocation capacity"))?;
                    } else if !entries.contains(&key) {
                        return Err(invalid());
                    }
                    Ok(())
                })
                .ok_or_else(invalid)??,
            _ => unreachable!(),
        }
        growth.commit();
        Ok(())
    }

    pub fn custom_remove(
        &self,
        collection: &Value,
        hash: i64,
        token: i64,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match collection {
            Value::Map(id) | Value::Set(id) => *id,
            _ => return Err(invalid()),
        };
        self.ensure_structure_mutable(id)?;
        self.ensure_key_mode(collection, true)?;
        let key = MapKey::custom(hash, token, Value::Unit);
        let removed = match collection {
            Value::Map(_) => self.with_map_mut(id, |entries| entries.remove(&key).is_some()),
            Value::Set(_) => self.with_set_mut(id, |entries| entries.remove(&key)),
            _ => None,
        }
        .ok_or_else(invalid)?;
        if removed {
            self.release_heap_units(1);
        }
        Ok(())
    }
}
