//! Admission and operations for fixed builtin arrays.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, GcObjectKind, HeapObjectId},
    native::storage_type::StorageType,
    value::Value,
};
use std::{ops::Bound, sync::Arc};

impl GcHeap {
    fn ensure_array(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.object_kind(id) == Some(GcObjectKind::Array) && self.buffer_contract(id).is_some() {
            Ok(())
        } else {
            Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "expected a builtin array object",
            ))
        }
    }

    pub fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.ensure_array(id).ok()?;
        self.buffer_len(id)
    }

    pub fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.ensure_array(id).ok()?;
        self.buffer_snapshot(id)
    }

    pub fn array_get(&self, id: HeapObjectId, index: impl TryInto<usize>) -> Option<Value> {
        self.ensure_array(id).ok()?;
        self.buffer_get(id, index)
    }

    pub fn array_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_element(id, index)
    }

    pub fn array_set(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_set(id, index, value)
    }

    pub(crate) fn array_contract(&self, id: HeapObjectId) -> Option<Arc<StorageType>> {
        self.ensure_array(id).ok()?;
        self.buffer_contract(id)
    }

    pub fn array_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_fill(id, value)
    }

    pub fn array_copy_from(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.ensure_array(source)?;
        self.buffer_copy_from(id, source)
    }

    pub fn array_copy_within(
        &self,
        id: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_copy_within(id, start, end, destination)
    }
}
