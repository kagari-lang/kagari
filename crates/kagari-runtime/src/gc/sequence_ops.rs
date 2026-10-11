use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::sequence::{SequencePayload, SequenceStorage},
    value::Value,
};
use kagari_types::ty::Ty;
use std::ops::Bound;

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid array target or index",
    )
}

impl GcHeap {
    pub fn sequence_push(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        self.validate_buffer_value(id, &value)?;
        let growth = self.resources.prepare_heap_growth(1)?;
        self.with_buffer_mut(id, |values| {
            values
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            values.push(value)
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }

    pub fn sequence_pop(&self, id: HeapObjectId) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_buffer_mut(id, SequenceStorage::pop)
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn sequence_insert(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        self.validate_buffer_value(id, &value)?;
        if index > self.buffer_len(id).ok_or_else(invalid)? {
            return Err(invalid());
        }
        let growth = self.resources.prepare_heap_growth(1)?;
        self.with_buffer_mut(id, |values| {
            values
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            values.insert(index, value)
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }

    pub fn sequence_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_buffer_mut(id, |values| values.remove(index))
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn sequence_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_buffer_mut(id, |values| {
                let length = values.len();
                values.clear();
                length
            })
            .ok_or_else(invalid)?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn prepare_sequence_removal(
        &self,
        target: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(target)?;
        let length = self.sequence_len(target).ok_or_else(invalid)?;
        let start = match start {
            Bound::Unbounded => 0,
            Bound::Included(n) => n,
            Bound::Excluded(n) => n.checked_add(1).ok_or_else(invalid)?,
        };
        let end = match end {
            Bound::Unbounded => length,
            Bound::Included(n) => n.checked_add(1).ok_or_else(invalid)?,
            Bound::Excluded(n) => n,
        };
        if start > end || end > length {
            return Err(invalid());
        }
        self.resources.poll_execution()?;
        let _removed_storage = self.resources.reserve_temporary_heap(end - start)?;
        let _remaining_storage = self
            .resources
            .reserve_temporary_heap(length - (end - start))?;
        let (removed, remaining) = self
            .with_buffer(target, |values| {
                Ok::<_, RuntimeError>((
                    values.copy_range(start, end)?,
                    values.copy_excluding(start, end)?,
                ))
            })
            .ok_or_else(invalid)??;
        let removed = Value::GcHandle(self.alloc_buffer_from(target, removed)?);
        let _root = self.root_value(removed).ok_or_else(invalid)?;
        let remaining = Value::GcHandle(self.alloc_buffer_from(target, remaining)?);
        self.alloc_tuple(vec![remaining, removed])
    }

    pub fn sequence_swap(&self, id: HeapObjectId, a: usize, b: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self.objects_mut()?;
        let revision = objects
            .get(id.index())
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if !matches!(object.ty, Ty::NativeObject(_)) {
            return Err(invalid());
        }
        let values = &mut object.payload_mut::<SequencePayload>()?.values;
        if a >= values.len() || b >= values.len() {
            return Err(invalid());
        }
        values.swap(a, b)?;
        if a != b {
            objects[id.index()].revision = revision;
        }
        Ok(())
    }

    pub fn sequence_reverse(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let count = self.sequence_len(id).ok_or_else(invalid)?;
        self.resources.poll_execution()?;
        let mut objects = self.objects_mut()?;
        let revision = objects
            .get(id.index())
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if !matches!(object.ty, Ty::NativeObject(_)) {
            return Err(invalid());
        }
        object.payload_mut::<SequencePayload>()?.values.reverse();
        if count > 1 {
            objects[id.index()].revision = revision;
        }
        Ok(())
    }

    pub fn sequence_truncate(&self, id: HeapObjectId, length: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let count = self.sequence_len(id).ok_or_else(invalid)?;
        let removed = count.saturating_sub(length);
        self.resources.poll_execution()?;
        self.with_buffer_mut(id, |values| values.truncate(length))
            .ok_or_else(invalid)?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn sequence_swap_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_buffer_mut(id, |values| values.swap_remove(index))
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn sequence_extend(
        &self,
        target: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(target)?;
        self.ensure_structure_mutable(target)?;
        let length = self.sequence_len(source).ok_or_else(invalid)?;
        let target_contract = self.sequence_contract(target).ok_or_else(invalid)?;
        let source_contract = self.sequence_contract(source).ok_or_else(invalid)?;
        if !source_contract.same_type(&target_contract) {
            return Err(invalid());
        }
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let prepared = self
            .with_buffer(source, |values| values.copy_range(0, length))
            .ok_or_else(invalid)??;
        self.ensure_execution_allowed()?;
        self.ensure_sequence(target)?;
        let growth = self.resources.prepare_heap_growth(length)?;
        self.with_buffer_mut(target, |values| {
            values
                .len()
                .checked_add(length)
                .ok_or_else(|| self.resource_limit("array length"))?;
            values
                .try_reserve(length)
                .map_err(|_| self.resource_limit("array capacity"))?;
            values.append_storage(&prepared)?;
            Ok::<_, RuntimeError>(())
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }
}
