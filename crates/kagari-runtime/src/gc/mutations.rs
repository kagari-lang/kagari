//! Guards prevent callbacks from modifying prepared mutation targets through aliases.

use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{CollectionIteration, GcHeap, GcObjectKind, HeapObjectId, storage::HeapObject},
    native::{
        hashed::{MapPayload, SetPayload},
        sequence::{SequencePayload, SequenceStorage},
    },
    value::Value,
};

use crate::native::hash_storage::{HashMapStorage, HashSetStorage};
use kagari_types::{scalar::BuiltinType, ty::Ty};

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
        let Some(HeapObject::Native(buffer_object)) = self.readable_object(&objects, *buffer)
        else {
            return Err(invalid());
        };
        if !matches!(buffer_object.ty, Ty::Array(..)) {
            return Err(invalid());
        }
        let input_payload = buffer_object.payload::<SequencePayload>()?;
        let input = &input_payload.values;
        let before = source.units();
        let _temporary = self.resources.reserve_temporary_heap(before)?;
        self.resources.poll_execution()?;
        let revision = objects[id.index()]
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let allocation = || self.resource_limit("prepared collection storage");
        let prepared = if operation == PreparedCollectionCommit::ReplaceArray {
            let HeapObject::Native(original) = source else {
                return Err(invalid());
            };
            if !matches!(original.ty, Ty::Array(..)) {
                return Err(invalid());
            }
            let payload = original.payload::<SequencePayload>()?;
            if payload.values.len() < input.len()
                || !input_payload.contract.same_type(&payload.contract)
            {
                return Err(invalid());
            }
            HeapObject::Native(original.replaced_payload(SequencePayload {
                leased_units: None,
                element: payload.element.clone(),
                contract: payload.contract.clone(),
                values: input.copy_range(0, input.len())?,
            })?)
        } else {
            if input_payload.element != Ty::Builtin(BuiltinType::Bool) {
                return Err(invalid());
            }
            if (0..input.len()).any(|index| !matches!(input.get(index), Some(Value::Bool(_)))) {
                return Err(invalid());
            }
            let kept = (0..input.len())
                .filter(|index| matches!(input.get(*index), Some(Value::Bool(true))))
                .count();
            match source {
                HeapObject::Native(original) if matches!(original.ty, Ty::Array(..)) => {
                    let payload = original.payload::<SequencePayload>()?;
                    if payload.values.len() != input.len() {
                        return Err(invalid());
                    }
                    let mut copy = SequenceStorage::empty(&payload.element);
                    copy.try_reserve(kept).map_err(|_| allocation())?;
                    for index in 0..input.len() {
                        if matches!(input.get(index), Some(Value::Bool(true))) {
                            copy.push(payload.values.get(index).ok_or_else(invalid)?)?;
                        }
                    }
                    HeapObject::Native(original.replaced_payload(SequencePayload {
                        leased_units: None,
                        element: payload.element.clone(),
                        contract: payload.contract.clone(),
                        values: copy,
                    })?)
                }
                HeapObject::Native(original) if matches!(original.ty, Ty::Map { .. }) => {
                    let payload = original.payload::<MapPayload>()?;
                    let values = &payload.entries;
                    if values.len() != input.len() {
                        return Err(invalid());
                    }
                    let mut copy = HashMapStorage::new();
                    copy.try_reserve(kept).map_err(|_| allocation())?;
                    for (index, (key, value)) in values.iter().enumerate() {
                        if matches!(input.get(index), Some(Value::Bool(true))) {
                            copy.insert(key.clone(), *value).map_err(|_| allocation())?;
                        }
                    }
                    HeapObject::Native(original.replaced_payload(MapPayload {
                        key: payload.key.clone(),
                        value: payload.value.clone(),
                        builtin_keys: payload.builtin_keys,
                        entries: copy,
                    })?)
                }
                HeapObject::Native(original) if matches!(original.ty, Ty::Set(..)) => {
                    let payload = original.payload::<SetPayload>()?;
                    let values = &payload.entries;
                    if values.len() != input.len() {
                        return Err(invalid());
                    }
                    let mut copy = HashSetStorage::new();
                    copy.try_reserve(kept).map_err(|_| allocation())?;
                    for (index, key) in values.iter().enumerate() {
                        if matches!(input.get(index), Some(Value::Bool(true))) {
                            copy.insert(key.clone()).map_err(|_| allocation())?;
                        }
                    }
                    HeapObject::Native(original.replaced_payload(SetPayload {
                        element: payload.element.clone(),
                        builtin_keys: payload.builtin_keys,
                        entries: copy,
                    })?)
                }
                _ => return Err(invalid()),
            }
        };
        let after = prepared.units();
        drop(objects);
        self.ensure_execution_allowed()?;
        let mut objects = self.objects_mut()?;
        *self.object_mut(&mut objects, id).ok_or_else(invalid)? = prepared;
        objects[id.index()].revision = revision;
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
        let root = self.root_value(*value).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid mutation root")
        })?;
        let lease = self
            .mutations
            .acquire(id, None)
            .map_err(|_| self.resource_limit("mutation registry"))?;
        Ok(CollectionIteration {
            _children: Vec::new(),
            loop_leases: Vec::new(),
            _lease: Some(lease),
            _root: root,
        })
    }

    pub(crate) fn ensure_callback_mutable(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.mutations.is_active(id) {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "container mutation during a guarded callback",
            ))
        } else {
            Ok(())
        }
    }
}
