//! Scoped access to one registered Rust payload. A borrow cannot escape its closure.
mod managed;
mod take;
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    frame::types::compatibility::TypeView,
    gc::{GcHeap, HeapObjectId, leases::BorrowedLease, storage::HeapObject},
    native::{
        binding::NativeResult,
        sequence::{NativeElement, SequencePayload},
        storage::{NativeObject, NativeStorage},
        stored_selection::StoredSelection,
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::{cell::Cell, sync::Arc};

struct NativeBorrow<'heap>(&'heap Cell<usize>);

impl Drop for NativeBorrow<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl GcHeap {
    pub(crate) fn with_native_data_mut<S: Send + 'static, R>(
        &self,
        id: HeapObjectId,
        edit: impl for<'payload> FnOnce(&'payload mut S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self.objects_mut()?;
        let Some(object) = self.native_ref(&objects, id) else {
            return Err(RuntimeError::module_validation(
                "invalid native edit receiver",
            ));
        };
        if !object.storage.is_editable_data() || !object.storage.accepts_payload::<S>() {
            return Err(RuntimeError::module_validation(
                "storage does not permit direct data editing",
            ));
        }
        // The sealed descriptor grants this flag only through data::<S: NativeData>.
        // Matching its Rust TypeId recovers that proof for erased loan adapters;
        // traced/managed/take-only storage never receives direct editing authority.
        let revision = object
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("native revision exhausted"))?;
        // Publish the revision before lending data so unwind preserves both the
        // completed writes and their invalidation. Size and graph edges are fixed.
        let Some(object) = self.native_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation(
                "invalid native edit receiver",
            ));
        };
        object.revision = revision;
        let data = object.payload_mut::<S>()?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        edit(data)
    }

    pub(crate) fn native_selections(
        &self,
        id: HeapObjectId,
    ) -> NativeResult<Arc<[StoredSelection]>> {
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        match self.readable_object(&objects, id) {
            Some(HeapObject::Native(object)) => Ok(object.selected.clone()),
            _ => Err(RuntimeError::module_validation(
                "invalid native selection receiver",
            )),
        }
    }

    pub(crate) fn try_enter_native_operation(
        &self,
        id: HeapObjectId,
    ) -> NativeResult<Option<BorrowedLease<'_>>> {
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        if !matches!(
            self.readable_object(&objects, id),
            Some(HeapObject::Native(_))
        ) {
            return Err(RuntimeError::module_validation(
                "invalid native operation receiver",
            ));
        }
        if self.native_operations.is_active(id) {
            return Ok(None);
        }
        self.native_operations.borrow(id).map(Some)
    }

    pub(crate) fn matches_native_type(&self, id: HeapObjectId, expected: TypeView<'_>) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if object.matches(expected))
    }

    pub(crate) fn default_storage(&self, ty: &Ty) -> Option<&NativeStorage> {
        match ty {
            Ty::Array(..) => Some(self.array_storage()),
            Ty::Map { .. } => Some(self.map_storage.get_or_init(|| NativeStorage::map(0, 1))),
            Ty::Set(..) => Some(self.set_storage.get_or_init(|| NativeStorage::set(0))),
            _ => None,
        }
    }

    pub(super) fn array_storage(&self) -> &NativeStorage {
        self.sequence_storage
            .get_or_init(|| NativeStorage::sequence(0))
    }

    pub(crate) fn native_type_name(&self, id: HeapObjectId) -> Option<String> {
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
        let Ty::NativeObject(nominal) = &object.ty else {
            return None;
        };
        Some(object.definition_name(nominal.declaration)?.to_owned())
    }

    pub(crate) fn ensure_no_native_borrow(&self) -> NativeResult<()> {
        if self.native_borrows.get() != 0 {
            return Err(RuntimeError::module_validation(
                "script reentry during native storage access",
            ));
        }
        Ok(())
    }

    pub(crate) fn alloc_native(&self, mut object: NativeObject) -> NativeResult<HeapObjectId> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        object
            .charge
            .logical_units
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("native object size"))?;
        // A copied payload can originate from an already mutated receiver.
        // Allocation creates a new identity; replacement does not pass here.
        object.revision = 0;
        self.alloc_object(HeapObject::Native(object))
    }

    pub(crate) fn with_native<S: Send + 'static, R>(
        &self,
        id: HeapObjectId,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(object) = self.native_ref(&objects, id) else {
            return Err(RuntimeError::module_validation(
                "invalid native storage receiver",
            ));
        };
        let payload = object.payload::<S>()?;
        if object
            .payload::<SequencePayload>()
            .is_ok_and(|sequence| sequence.detached)
        {
            return Err(RuntimeError::module_validation(
                "sequence storage is exclusively borrowed",
            ));
        }
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        access(payload)
    }

    pub(crate) fn push_sequence_value(
        &self,
        id: HeapObjectId,
        expected: TypeView<'_>,
        value: Value,
    ) -> NativeResult<()> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        {
            let objects = self.objects.try_borrow().map_err(|_| {
                RuntimeError::module_validation("conflicting native storage borrow")
            })?;
            let Some(object) = self.native_ref(&objects, id) else {
                return Err(RuntimeError::module_validation("invalid sequence receiver"));
            };
            let sequence = object.payload::<SequencePayload>()?;
            if !object.matches(expected)
                || !self.valid_payload(&value)
                || !sequence.contract.accepts_value(self, &value)
            {
                return Err(RuntimeError::module_validation(
                    "sequence value differs from its element type",
                ));
            }
        }
        self.with_buffer_mut(id, 1, |values| {
            values
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            values.push(value)
        })?
    }

    pub(crate) fn with_sequence_mut<E: NativeElement, R>(
        &self,
        id: HeapObjectId,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        self.ensure_sequence(id)?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self
            .objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(object) = self.native_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation("invalid sequence receiver"));
        };
        let revision = object
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("sequence revision exhausted"))?;
        let sequence = object.payload_mut::<SequencePayload>()?;
        let values = E::slice_mut(&mut sequence.values)
            .ok_or_else(|| RuntimeError::module_validation("sequence scalar layout"))?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        let result = access(values);
        // A bulk write may permute slots. Closed cursors must observe that change
        // even when the closure returned an error after completed scalar writes.
        object.revision = revision;
        result
    }
}
