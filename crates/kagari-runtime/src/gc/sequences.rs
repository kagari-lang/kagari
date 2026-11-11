//! Family admission for registered growable sequences.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, GcObjectKind, HeapObjectId},
    native::storage_type::StorageType,
    value::Value,
};

use std::{ops::Bound, sync::Arc};

impl GcHeap {
    pub(crate) fn ensure_sequence(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.object_kind(id) == Some(GcObjectKind::Native) && self.buffer_contract(id).is_some()
        {
            Ok(())
        } else {
            Err(RuntimeError::module_validation(
                "expected a nominal sequence object",
            ))
        }
    }

    pub fn sequence_len(&self, id: HeapObjectId) -> Option<usize> {
        self.ensure_sequence(id).ok()?;
        self.buffer_len(id)
    }

    pub fn sequence_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.ensure_sequence(id).ok()?;
        self.buffer_snapshot(id)
    }

    pub fn sequence_get(&self, id: HeapObjectId, index: impl TryInto<usize>) -> Option<Value> {
        self.ensure_sequence(id).ok()?;
        self.buffer_get(id, index)
    }

    pub fn sequence_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_element(id, index)
    }

    pub fn sequence_set(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_set(id, index, value)
    }

    pub(crate) fn sequence_contract(&self, id: HeapObjectId) -> Option<Arc<StorageType>> {
        self.ensure_sequence(id).ok()?;
        self.buffer_contract(id)
    }

    pub fn sequence_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_fill(id, value)
    }

    pub fn sequence_copy_from(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.ensure_sequence(source)?;
        self.buffer_copy_from(id, source)
    }

    pub fn sequence_copy_within(
        &self,
        id: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.buffer_copy_within(id, start, end, destination)
    }

    pub(crate) fn check_sequence_replacement(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<(), RuntimeError> {
        self.ensure_sequence(id)?;
        self.check_buffer_replacement(id, index)
    }
}
