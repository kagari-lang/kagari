use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObject, HeapObjectId},
    value::Value,
};
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
        let (mut removed, _removed_storage) = self.prepare_array_copy(end - start)?;
        let (mut remaining, _remaining_storage) =
            self.prepare_array_copy(length - (end - start))?;
        self.with_array(target, |values| {
            removed.extend(values[start..end].iter().cloned());
            remaining.extend(values[..start].iter().chain(&values[end..]).cloned());
        })
        .ok_or_else(invalid)?;
        let removed = Value::Array(self.alloc_array(removed)?);
        let _root = self.root_value(removed.clone()).ok_or_else(invalid)?;
        let remaining = Value::Array(self.alloc_array(remaining)?);
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
        let Some(HeapObject::Array(values)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        if a >= values.len() || b >= values.len() {
            return Err(invalid());
        }
        values.swap(a, b);
        if a != b {
            objects[id.slot].revision = revision;
        }
        Ok(())
    }
    pub fn array_reverse(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let count = self.array_len(id).ok_or_else(invalid)?;
        self.resources.consume_instruction_steps(count as u64)?;
        let mut objects = self.objects.borrow_mut();
        let revision = objects
            .get(id.slot)
            .ok_or_else(invalid)?
            .revision
            .checked_add(1)
            .ok_or_else(invalid)?;
        let Some(HeapObject::Array(values)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        values.reverse();
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
        self.resources.consume_instruction_steps(removed as u64)?;
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
            .with_array_mut(id, |values| {
                (index < values.len()).then(|| values.swap_remove(index))
            })
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
        let (mut prepared, _temporary) = self.prepare_array_copy(length)?;
        self.with_array(source, |values| prepared.extend(values.iter().cloned()))
            .ok_or_else(invalid)?;
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
            values.extend(prepared);
            Ok::<_, RuntimeError>(())
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }
}
