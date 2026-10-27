//! Scoped access to one registered Rust payload. A borrow cannot escape its closure.
use crate::gc::array_storage::ArrayPayload;
mod access;
mod managed;
#[cfg(test)]
mod protocol_tests;
mod take;
#[cfg(test)]
mod tests;
use crate::{
    error::RuntimeError,
    frame::types::{arguments::TypeArgument, compatibility::TypeView},
    gc::{GcHeap, HeapObjectId, leases::BorrowedLease, storage::HeapObject},
    module::ExecutableModule,
    native::{
        binding::NativeResult,
        storage::{NativeObject, NativeStorage},
        stored_selection::StoredSelection,
        views::{NativeOperationGuard, NativeOperationKind},
    },
    value::Value,
};
use kagari_types::ty::Ty;
use std::{any::Any, cell::Cell, sync::Arc};

struct NativeBorrow<'heap>(&'heap Cell<usize>);

impl Drop for NativeBorrow<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl GcHeap {
    pub(crate) fn scan_native_payload<'a>(
        &self,
        storage: &NativeStorage,
        payload: &'a dyn Any,
        visit: &mut dyn FnMut(&'a Value),
    ) -> NativeResult<()> {
        self.ensure_execution_allowed()?;
        self.native_scan_workspace
            .try_borrow_mut()
            .map_err(|_| {
                RuntimeError::module_validation("conflicting native scan workspace borrow")
            })?
            .reserve(storage.scan_layout())?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        storage.scan_payload(payload, visit)
    }

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
        kind: NativeOperationKind,
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
        if self.native_operations.is_active(id)
            && !(kind == NativeOperationKind::ReadOnlyCallbacks
                && self.native_operations.operation(id)
                    == Some(NativeOperationKind::ReadOnlyCallbacks))
        {
            return Ok(None);
        }
        self.native_operations.borrow_operation(id, kind).map(Some)
    }

    pub(crate) fn matches_native_argument(
        &self,
        id: HeapObjectId,
        expected: &TypeArgument,
        owner: &ExecutableModule,
    ) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if object.matches_argument(expected, owner))
    }

    pub(crate) fn matches_native_type(&self, id: HeapObjectId, expected: TypeView<'_>) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if object.matches(expected))
    }

    pub(crate) fn default_storage(&self, ty: &Ty) -> Option<&NativeStorage> {
        match ty {
            Ty::Array(..) => Some(self.array_storage()),

            _ => None,
        }
    }

    pub(super) fn array_storage(&self) -> &NativeStorage {
        self.array_storage
            .get_or_init(NativeStorage::payload::<ArrayPayload>)
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

    pub(crate) fn check_native_payload_argument<P: 'static>(
        &self,
        id: HeapObjectId,
        expected: &TypeArgument,
        owner: &ExecutableModule,
    ) -> NativeResult<()> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(RuntimeError::module_validation("invalid native identity"));
        };
        if !object.matches_argument(expected, owner) {
            return Err(RuntimeError::module_validation(
                "native argument differs from its declared type",
            ));
        }
        if !object.storage.accepts_payload::<P>() {
            return Err(RuntimeError::module_validation(
                "storage does not permit fixed-data loan",
            ));
        }
        Ok(())
    }

    pub(crate) fn check_native_payload<S: 'static>(
        &self,
        id: HeapObjectId,
        data_only: bool,
    ) -> NativeResult<()> {
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(RuntimeError::module_validation("invalid native identity"));
        };
        if !object.storage.accepts_payload::<S>()
            || (data_only && !object.storage.is_editable_data())
        {
            return Err(RuntimeError::module_validation(
                "storage does not permit fixed-data loan",
            ));
        }
        Ok(())
    }

    pub(crate) fn native_operation_kind(&self, id: HeapObjectId) -> Option<NativeOperationKind> {
        self.native_operations.operation(id)
    }

    pub(crate) fn with_native<S: Send + 'static, R>(
        &self,
        id: HeapObjectId,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.with_native_operation(id, None, access)
    }

    pub(crate) fn with_native_operation<S: Send + 'static, R>(
        &self,
        id: HeapObjectId,
        operation: Option<&NativeOperationGuard<'_>>,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        self.check_native_operation(id, operation, false)?;
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

        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        access(payload)
    }
}
