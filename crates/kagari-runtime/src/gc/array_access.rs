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

    /// Query checked heap metadata for protected backend transport.
    ///
    /// # Safety
    /// Keep managed inputs live through this query. Type/owner validation cannot
    /// establish the lifetime of an expired direct reference.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_len_transport(&self, id: HeapObjectId) -> Option<usize> {
        self.array_len(id)
    }

    pub(crate) fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.ensure_array(id).ok()?;
        self.buffer_len(id)
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_snapshot_transport(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.array_snapshot(id)
    }

    pub(crate) fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.ensure_array(id).ok()?;
        self.buffer_snapshot(id)
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_get_transport(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Option<Value> {
        self.array_get(id, index)
    }

    pub(crate) fn array_get(&self, id: HeapObjectId, index: impl TryInto<usize>) -> Option<Value> {
        self.ensure_array(id).ok()?;
        self.buffer_get(id, index)
    }

    /// Read a checked backend snapshot without adding independent retention.
    ///
    /// # Safety
    /// Keep source objects live through the read. Protect copied managed edges
    /// or identities before callbacks, collection or source mutation can remove
    /// their last traced owner. A returned snapshot is not a root.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_element_transport(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.array_element(id, index)
    }

    pub(crate) fn array_element(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_element(id, index)
    }

    /// Mutate backend storage through existing type, bounds and publication checks.
    ///
    /// # Safety
    /// Keep the target and every managed input live through checked publication
    /// and commit. Preserve source protection across preparation; an unrooted copy
    /// cannot be used after callbacks or mutation remove its last traced owner.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_set_transport(
        &self,
        id: HeapObjectId,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.array_set(id, index, value)
    }

    pub(crate) fn array_set(
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

    /// Mutate backend storage through existing type, bounds and publication checks.
    ///
    /// # Safety
    /// Keep the target and every managed input live through checked publication
    /// and commit. Preserve source protection across preparation; an unrooted copy
    /// cannot be used after callbacks or mutation remove its last traced owner.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_fill_transport(
        &self,
        id: HeapObjectId,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.array_fill(id, value)
    }

    pub(crate) fn array_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.buffer_fill(id, value)
    }

    /// Mutate backend storage through existing type, bounds and publication checks.
    ///
    /// # Safety
    /// Keep the target and every managed input live through checked publication
    /// and commit. Preserve source protection across preparation; an unrooted copy
    /// cannot be used after callbacks or mutation remove its last traced owner.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_copy_from_transport(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.array_copy_from(id, source)
    }

    pub(crate) fn array_copy_from(
        &self,
        id: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_array(id)?;
        self.ensure_array(source)?;
        self.buffer_copy_from(id, source)
    }

    /// Mutate backend storage through existing type, bounds and publication checks.
    ///
    /// # Safety
    /// Keep the target and every managed input live through checked publication
    /// and commit. Preserve source protection across preparation; an unrooted copy
    /// cannot be used after callbacks or mutation remove its last traced owner.
    /// With the current checked-ID representation, invalid-ID rejection probes
    /// must remain non-dereferencing; replace them with inert identity or retained
    /// wrapper checks before converting these paths to direct references.
    #[doc(hidden)]
    pub unsafe fn array_copy_within_transport(
        &self,
        id: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.array_copy_within(id, start, end, destination)
    }

    pub(crate) fn array_copy_within(
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
