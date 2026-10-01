//! Guards prevent callbacks from modifying prepared mutation targets through aliases.

use crate::error::RuntimeError;
use crate::{
    error::RuntimeErrorKind,
    gc::{CollectionIteration, GcHeap, GcObjectKind, HeapObject, HeapObjectId},
    value::Value,
};
use indexmap::IndexMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreparedCollectionCommit {
    ReplaceArray,
    Retain,
}

impl GcHeap {
    pub fn commit_prepared_collection(
        &self,
        operation: PreparedCollectionCommit,
        args: &[Value],
    ) -> Result<(), RuntimeError> {
        let invalid =
            || RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid prepared collection");
        let [target, Value::Array(buffer)] = args else {
            return Err(invalid());
        };
        let id = match target {
            Value::Array(id) | Value::Map(id) | Value::Set(id) => *id,
            _ => return Err(invalid()),
        };
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let objects = self.objects.borrow();
        let source = self.readable_object(&objects, id).ok_or_else(invalid)?;
        let Some(HeapObject::Array(input)) = self.readable_object(&objects, *buffer) else {
            return Err(invalid());
        };
        let before = source.units();
        let _temporary = self.resources.reserve_temporary_heap(before)?;
        self.resources.consume_instruction_steps(before as u64)?;
        let revision = objects[id.slot]
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let allocation = || self.resource_limit("prepared collection storage");
        let prepared = if operation == PreparedCollectionCommit::ReplaceArray {
            let HeapObject::Array(original) = source else {
                return Err(invalid());
            };
            if original.len() < input.len() {
                return Err(invalid());
            }
            let mut copy = Vec::new();
            copy.try_reserve_exact(input.len())
                .map_err(|_| allocation())?;
            copy.extend(input.iter().cloned());
            HeapObject::Array(copy)
        } else {
            if input.iter().any(|v| !matches!(v, Value::Bool(_))) {
                return Err(invalid());
            }
            let kept = input
                .iter()
                .filter(|v| matches!(v, Value::Bool(true)))
                .count();
            match source {
                HeapObject::Array(values) if values.len() == input.len() => {
                    let mut copy = Vec::new();
                    copy.try_reserve_exact(kept).map_err(|_| allocation())?;
                    copy.extend(
                        values
                            .iter()
                            .zip(input)
                            .filter(|(_, keep)| matches!(keep, Value::Bool(true)))
                            .map(|(v, _)| v.clone()),
                    );
                    HeapObject::Array(copy)
                }
                HeapObject::Map(values) if values.len() == input.len() => {
                    let mut copy = IndexMap::new();
                    copy.try_reserve(kept).map_err(|_| allocation())?;
                    for ((key, value), keep) in values.iter().zip(input) {
                        if matches!(keep, Value::Bool(true)) {
                            copy.insert(key.clone(), value.clone());
                        }
                    }
                    HeapObject::Map(copy)
                }
                HeapObject::Set(values) if values.len() == input.len() => {
                    let mut copy = IndexMap::new();
                    copy.try_reserve(kept).map_err(|_| allocation())?;
                    for ((key, ()), keep) in values.iter().zip(input) {
                        if matches!(keep, Value::Bool(true)) {
                            copy.insert(key.clone(), ());
                        }
                    }
                    HeapObject::Set(copy)
                }
                _ => return Err(invalid()),
            }
        };
        let after = prepared.units();
        drop(objects);
        self.ensure_execution_allowed()?;
        let mut objects = self.objects.borrow_mut();
        *self.object_mut(&mut objects, id).ok_or_else(invalid)? = prepared;
        objects[id.slot].revision = revision;
        self.release_heap_units(before - after);
        Ok(())
    }

    pub(crate) fn begin_collection_mutation(
        &self,
        value: &Value,
    ) -> Result<CollectionIteration, RuntimeError> {
        self.ensure_execution_allowed()?;
        let id = match value {
            Value::Array(id) if self.object_kind(*id) == Some(GcObjectKind::Array) => *id,
            Value::Map(id) if self.object_kind(*id) == Some(GcObjectKind::Map) => *id,
            Value::Set(id) if self.object_kind(*id) == Some(GcObjectKind::Set) => *id,
            _ => {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid mutation target",
                ));
            }
        };
        self.ensure_callback_mutable(id)?;
        let root = self.root_value(value.clone()).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid mutation root")
        })?;
        let mut active = self.mutations.borrow_mut();
        active
            .try_reserve(1)
            .map_err(|_| self.resource_limit("mutation registry"))?;
        active.insert(id, 1);
        Ok(CollectionIteration {
            _children: Vec::new(),
            iter_loops: Vec::new(),
            active: self.mutations.clone(),
            id: Some(id),
            _root: root,
        })
    }

    pub(crate) fn ensure_callback_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.mutations.borrow().contains_key(&id) {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "container mutation during a guarded callback",
            ))
        } else {
            Ok(())
        }
    }
}
