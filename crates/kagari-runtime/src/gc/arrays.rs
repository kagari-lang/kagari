use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObject, HeapObjectId},
    resource::TemporaryHeap,
    value::Value,
};
use std::ops::Bound;

impl GcHeap {
    pub fn alloc_array(&self, elements: Vec<Value>) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !elements.iter().all(|value| self.valid_payload(value)) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.alloc_object(HeapObject::Array(elements))
    }

    pub fn alloc_array_repeat(
        &self,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid repeat array value",
            ));
        }
        self.resources.consume_instruction_steps(count as u64)?;
        // Check the final allocation before reserving host storage.
        let units = count
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("array length"))?;
        drop(self.resources.prepare_heap_growth(units)?);
        let mut elements = Vec::new();
        elements
            .try_reserve_exact(count)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        for index in 0..count {
            if index % 1024 == 0 {
                self.ensure_execution_allowed()?;
            }
            elements.push(value.clone());
        }
        self.alloc_array(elements)
    }

    pub fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_array(id, |elements| elements.len())
    }

    pub fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.with_array(id, |elements| elements.clone())
    }

    pub fn array_get(&self, id: HeapObjectId, index: usize) -> Option<Value> {
        self.with_array(id, |elements| elements.get(index).cloned())
            .flatten()
    }

    pub fn array_push(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            let growth = self.resources.prepare_heap_growth(1)?;
            elements
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            elements.push(value);
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn array_pop(&self, id: HeapObjectId) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |elements| elements.pop())
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_insert(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap target, index, or payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            if index > elements.len() {
                return Err(RuntimeError::new(
                    RuntimeErrorKind::ScriptTrap,
                    "invalid heap target, index, or payload",
                ));
            }
            let growth = self.resources.prepare_heap_growth(1)?;
            elements
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            elements.insert(index, value);
            growth.commit();
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub fn array_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |elements| {
                (index < elements.len()).then(|| elements.remove(index))
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_array_mut(id, |elements| {
                let removed = elements.len();
                elements.clear();
                removed
            })
            .ok_or_else(|| {
                RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target")
            })?;
        self.release_heap_units(removed);
        Ok(())
    }

    /// Prepare all shallow copies before replacing any target slot.
    pub fn array_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid array fill payload",
            ));
        }
        let length = self.array_len(id).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid array target")
        })?;
        let (mut prepared, _temporary) = self.prepare_array_copy(length)?;
        for index in 0..length {
            if index % 1024 == 0 {
                self.ensure_execution_allowed()?;
            }
            prepared.push(value.clone());
        }
        self.commit_array_copy(id, prepared)
    }

    pub fn array_copy_from(
        &self,
        target: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(target)?;
        let length = self.array_len(target).ok_or_else(|| {
            RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid array target")
        })?;
        if self.array_len(source) != Some(length) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "array copy requires equal lengths",
            ));
        }
        let (mut prepared, _temporary) = self.prepare_array_copy(length)?;
        self.with_array(source, |values| {
            for (index, value) in values.iter().enumerate() {
                if index % 1024 == 0 {
                    self.ensure_execution_allowed()?;
                }
                prepared.push(value.clone());
            }
            Ok::<_, RuntimeError>(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid array source"))??;
        self.commit_array_copy(target, prepared)
    }

    pub fn array_copy_within(
        &self,
        target: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(target)?;
        let invalid = || {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "array copy range is out of bounds",
            )
        };
        let length = self.array_len(target).ok_or_else(invalid)?;
        let start = match start {
            Bound::Unbounded => 0,
            Bound::Included(n) => n,
            Bound::Excluded(n) => n.checked_add(1).ok_or_else(invalid)?,
        };
        let end = match end {
            Bound::Unbounded => length,
            Bound::Excluded(n) => n,
            Bound::Included(n) => n.checked_add(1).ok_or_else(invalid)?,
        };
        if start > end || end > length || destination > length || end - start > length - destination
        {
            return Err(invalid());
        }
        let (mut prepared, _temporary) = self.prepare_array_copy(end - start)?;
        self.with_array(target, |values| {
            for (index, value) in values[start..end].iter().enumerate() {
                if index % 1024 == 0 {
                    self.ensure_execution_allowed()?;
                }
                prepared.push(value.clone());
            }
            Ok::<_, RuntimeError>(())
        })
        .ok_or_else(invalid)??;
        self.ensure_execution_allowed()?;
        self.with_array_mut(target, |values| {
            for (slot, value) in values[destination..destination + prepared.len()]
                .iter_mut()
                .zip(prepared)
            {
                *slot = value;
            }
        })
        .ok_or_else(invalid)
    }

    pub(super) fn prepare_array_copy(
        &self,
        length: usize,
    ) -> Result<(Vec<Value>, TemporaryHeap<'_>), RuntimeError> {
        self.resources.consume_instruction_steps(length as u64)?;
        // Temporary copies must fit the session's allocation and memory limits.
        let temporary = self.resources.reserve_temporary_heap(length)?;
        let mut prepared = Vec::new();
        prepared
            .try_reserve_exact(length)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        Ok((prepared, temporary))
    }

    pub(super) fn commit_array_copy(
        &self,
        target: HeapObjectId,
        prepared: Vec<Value>,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(target)?;
        self.with_array_mut(target, |values| {
            debug_assert_eq!(values.len(), prepared.len());
            *values = prepared;
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid array target"))
    }

    pub fn array_set(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(id)?;
        if !self.valid_payload(&value) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "invalid heap payload",
            ));
        }
        self.with_array_mut(id, |elements| {
            let slot = elements.get_mut(index).ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::IndexOutOfBounds,
                    format!("invalid index `{index}`"),
                )
            })?;
            *slot = value;
            Ok(())
        })
        .ok_or_else(|| RuntimeError::new(RuntimeErrorKind::ScriptTrap, "invalid heap target"))?
    }

    pub(crate) fn with_array<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&Vec<Value>) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        match self.readable_object(&objects, id)? {
            HeapObject::Array(elements) => Some(f(elements)),
            HeapObject::Map(_) | HeapObject::Set(_) | HeapObject::Enum(..) => None,
            HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::IteratorCapture(_) => None,
        }
    }

    pub(super) fn with_array_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut Vec<Value>) -> R,
    ) -> Option<R> {
        let mut objects = self.objects.borrow_mut();
        let revision = objects.get(id.slot)?.revision.checked_add(1)?;
        match self.object_mut(&mut objects, id)? {
            HeapObject::Array(elements) => {
                let old_len = elements.len();
                let result = f(elements);
                if elements.len() != old_len {
                    objects[id.slot].revision = revision;
                }
                Some(result)
            }
            HeapObject::Map(_) | HeapObject::Set(_) | HeapObject::Enum(..) => None,
            HeapObject::Struct { .. }
            | HeapObject::Interface { .. }
            | HeapObject::Closure { .. }
            | HeapObject::Cell { .. }
            | HeapObject::Iter(_)
            | HeapObject::IteratorCapture(_) => None,
        }
    }
}
