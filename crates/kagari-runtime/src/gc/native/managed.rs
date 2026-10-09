//! Checked writes to declared native fields. The collector only sees their trace.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, native::NativeBorrow, storage::HeapObject},
    native::{
        binding::NativeResult,
        payload::{
            data::NativeData,
            managed::{AppliedSchema, Managed, invalid},
        },
    },
    value::Value,
};

impl GcHeap {
    /// Only this internal commit helper exposes the whole managed representation.
    /// Callers validate new edges before entering; user edits receive data alone.
    fn with_managed_mut<T: NativeData, R>(
        &self,
        id: HeapObjectId,
        commit: impl FnOnce(&mut Managed<T>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self.objects_mut()?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, id) else {
            return Err(invalid());
        };
        let payload = object.payload::<Managed<T>>()?;
        if !payload.schema.registered_in(&object.storage) {
            return Err(invalid());
        }
        let revision = objects[id.index()]
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("native revision exhausted"))?;
        objects[id.index()].revision = revision;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(invalid());
        };
        let payload = object.payload_mut::<Managed<T>>()?;
        self.native_borrows.set(self.native_borrows.get() + 1);
        let _borrow = NativeBorrow(&self.native_borrows);
        commit(payload)
    }

    pub(crate) fn edit_managed_data<T: NativeData, R>(
        &self,
        id: HeapObjectId,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.with_managed_mut(id, |payload: &mut Managed<T>| edit(&mut payload.data))
    }

    pub(crate) fn set_managed_field<T: NativeData>(
        &self,
        id: HeapObjectId,
        schema: &AppliedSchema,
        slot: usize,
        value: Value,
    ) -> NativeResult<()> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        let field = schema.fields.get(slot).ok_or_else(invalid)?;
        if !value.is_default_heap_payload(self) || !field.contract.accepts_value(self, &value) {
            return Err(invalid());
        }
        self.with_native::<Managed<T>, _>(id, |payload| {
            if payload.schema.matches(schema) && slot < payload.values.len() {
                Ok(())
            } else {
                Err(invalid())
            }
        })?;
        self.with_managed_mut(id, |payload: &mut Managed<T>| {
            // The old edge is still present here. A future write barrier belongs
            // at this boundary, before dropping it and publishing the new edge.
            payload.values[slot] = value;
            Ok(())
        })
    }

    pub(crate) fn replace_managed<T: NativeData>(
        &self,
        id: HeapObjectId,
        replacement: Managed<T>,
    ) -> NativeResult<()> {
        self.ensure_no_native_borrow()?;
        self.ensure_execution_allowed()?;
        if replacement.values.len() != replacement.schema.fields.len() {
            return Err(invalid());
        }
        for (field, value) in replacement.schema.fields.iter().zip(&replacement.values) {
            if !value.is_default_heap_payload(self) || !field.contract.accepts_value(self, value) {
                return Err(invalid());
            }
        }
        self.with_native::<Managed<T>, _>(id, |payload| {
            if payload.schema.matches(&replacement.schema)
                && payload.values.len() == replacement.values.len()
            {
                Ok(())
            } else {
                Err(invalid())
            }
        })?;
        self.with_managed_mut(id, |payload: &mut Managed<T>| {
            // All old and new edges are available before the single commit.
            // Data size and field count remain unchanged, including on unwind.
            *payload = replacement;
            Ok(())
        })
    }
}
