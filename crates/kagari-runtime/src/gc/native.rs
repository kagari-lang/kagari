//! Scoped access to one registered Rust payload. A borrow cannot escape its closure.
use crate::{
    error::RuntimeError,
    frame::types::{TypeEnvironment, compatibility::TypeView},
    gc::{GcHeap, HeapObject, HeapObjectId},
    module::LoadedModule,
    native::{
        binding::NativeResult,
        sequence::{NativeElement, SequencePayload},
        storage::{NativeObject, NativePayload, NativeStorage},
    },
    value::Value,
};
use kagari_abi::types::AbiType;
use kagari_common::identity::table::DefinitionId;
use std::cell::Cell;

struct NativeBorrow<'heap>(&'heap Cell<usize>);

impl Drop for NativeBorrow<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl GcHeap {
    pub(crate) fn matches_native_type(
        &self,
        id: HeapObjectId,
        ty: &AbiType<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> bool {
        let objects = self.objects.borrow();
        matches!(self.readable_object(&objects, id), Some(HeapObject::Native(object)) if object.matches(ty, owner, environment))
    }

    pub(crate) fn default_storage(&self, ty: &AbiType<DefinitionId>) -> Option<&NativeStorage> {
        match ty {
            AbiType::Array(..) => Some(&self.sequence_storage),
            AbiType::Map { .. } => Some(&self.map_storage),
            AbiType::Set(..) => Some(&self.set_storage),
            _ => None,
        }
    }

    pub(crate) fn native_type_name(&self, id: HeapObjectId) -> Option<String> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        let AbiType::NativeObject(nominal) = &object.ty else {
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

    pub(crate) fn alloc_native(&self, object: NativeObject) -> NativeResult<HeapObjectId> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        object
            .units()
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("native object size"))?;
        self.alloc_object(HeapObject::Native(object))
    }

    pub(crate) fn with_native<S: NativePayload, R>(
        &self,
        id: HeapObjectId,
        access: impl for<'payload> FnOnce(&'payload S) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(RuntimeError::module_validation(
                "invalid native storage receiver",
            ));
        };
        let payload = object.payload::<S>()?;
        if object
            .payload::<SequencePayload>()
            .is_ok_and(|sequence| sequence.leased_units.is_some())
        {
            return Err(RuntimeError::module_validation(
                "sequence storage is exclusively borrowed",
            ));
        }
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        access(payload)
    }

    pub(crate) fn sequence_push(
        &self,
        id: HeapObjectId,
        expected: TypeView<'_>,
        value: Value,
    ) -> NativeResult<()> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        {
            let objects = self.objects.try_borrow().map_err(|_| {
                RuntimeError::module_validation("conflicting native storage borrow")
            })?;
            let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
                return Err(RuntimeError::module_validation("invalid sequence receiver"));
            };
            let sequence = object.payload::<SequencePayload>()?;
            if !object.matches(expected.ty, expected.owner, expected.environment)
                || !self.valid_payload(&value)
                || !sequence.contract.accepts_value(self, &value)
            {
                return Err(RuntimeError::module_validation(
                    "sequence value differs from its element type",
                ));
            }
        }
        let growth = self.resources.prepare_heap_growth(1)?;
        let mut objects = self
            .objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation("invalid sequence receiver"));
        };
        let sequence = object.payload_mut::<SequencePayload>()?;
        sequence
            .values
            .try_reserve(1)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        sequence.values.push(value)?;
        growth.commit();
        Ok(())
    }

    pub(crate) fn with_sequence_mut<E: NativeElement, R>(
        &self,
        id: HeapObjectId,
        access: impl for<'slice> FnOnce(&'slice mut [E]) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self
            .objects
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("conflicting native storage borrow"))?;
        let revision = objects
            .get(id.slot)
            .ok_or_else(|| RuntimeError::module_validation("invalid sequence receiver"))?
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("sequence revision exhausted"))?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation("invalid sequence receiver"));
        };
        let sequence = object.payload_mut::<SequencePayload>()?;
        let values = E::slice_mut(&mut sequence.values)
            .ok_or_else(|| RuntimeError::module_validation("sequence scalar layout"))?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        let result = access(values);
        // A bulk write may permute slots. Closed cursors must observe that change
        // even when the closure returned an error after completed scalar writes.
        objects[id.slot].revision = revision;
        result
    }
}
