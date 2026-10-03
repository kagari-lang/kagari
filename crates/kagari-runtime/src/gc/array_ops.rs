use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObject, HeapObjectId},
    native::sequence::SequencePayload,
    value::Value,
};
use kagari_abi::types::AbiType;
use std::ops::Bound;

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid array target or index",
    )
}

impl GcHeap {
    pub fn prepare_array_removal(
        &self,
        target: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
    ) -> Result<Value, RuntimeError> {
        self.ensure_execution_allowed()?;
        let length = self.array_len(target).ok_or_else(invalid)?;
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
            .with_array(target, |values| {
                Ok::<_, RuntimeError>((
                    values.copy_range(start, end)?,
                    values.copy_excluding(start, end)?,
                ))
            })
            .ok_or_else(invalid)??;
        let removed = Value::Array(self.alloc_array_from(target, removed)?);
        let _root = self.root_value(removed.clone()).ok_or_else(invalid)?;
        let remaining = Value::Array(self.alloc_array_from(target, remaining)?);
        Ok(Value::Tuple(vec![remaining, removed]))
    }

    pub fn array_swap(&self, id: HeapObjectId, a: usize, b: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self.objects.borrow_mut();
        let revision = objects
            .get(id.slot)
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if !matches!(object.ty, AbiType::Array(..)) {
            return Err(invalid());
        }
        let values = &mut object.payload_mut::<SequencePayload>()?.values;
        if a >= values.len() || b >= values.len() {
            return Err(invalid());
        }
        values.swap(a, b)?;
        if a != b {
            objects[id.slot].revision = revision;
        }
        Ok(())
    }

    pub fn array_reverse(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let count = self.array_len(id).ok_or_else(invalid)?;
        self.resources.poll_execution()?;
        let mut objects = self.objects.borrow_mut();
        let revision = objects
            .get(id.slot)
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if !matches!(object.ty, AbiType::Array(..)) {
            return Err(invalid());
        }
        object.payload_mut::<SequencePayload>()?.values.reverse();
        if count > 1 {
            objects[id.slot].revision = revision;
        }
        Ok(())
    }

    pub fn array_truncate(&self, id: HeapObjectId, length: usize) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let count = self.array_len(id).ok_or_else(invalid)?;
        let removed = count.saturating_sub(length);
        self.resources.poll_execution()?;
        self.with_array_mut(id, |values| values.truncate(length))
            .ok_or_else(invalid)?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn array_swap_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |values| values.swap_remove(index))
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_extend(
        &self,
        target: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(target)?;
        let length = self.array_len(source).ok_or_else(invalid)?;
        let target_contract = self.array_contract(target).ok_or_else(invalid)?;
        let source_contract = self.array_contract(source).ok_or_else(invalid)?;
        if !source_contract.same_type(&target_contract) {
            return Err(invalid());
        }
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let prepared = self
            .with_array(source, |values| values.copy_range(0, length))
            .ok_or_else(invalid)??;
        self.ensure_execution_allowed()?;
        let growth = self.resources.prepare_heap_growth(length)?;
        self.with_array_mut(target, |values| {
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
