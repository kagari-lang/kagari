//! Checked writes to declared native fields. The collector only sees their trace.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, leases::StorageBorrow, reference::GcRef},
    native::{
        binding::NativeResult,
        payload::{
            data::NativeData,
            managed::{AppliedSchema, Managed, invalid},
        },
        storage::{NativePayload, value::StoredValue},
    },
    value::Value,
};

impl GcHeap {
    /// Only this internal commit helper exposes the whole managed representation.
    /// Callers validate new edges before entering; user edits receive data alone.
    fn with_managed_mut<T: NativeData, R>(
        &self,
        id: GcRef,
        commit: impl FnOnce(&mut Managed<T>) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.ensure_no_storage_borrow()?;
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let mut objects = self.objects_mut()?;
        let Some(object) = self.native_ref(&objects, id) else {
            return Err(invalid());
        };
        let payload = object.payload::<Managed<T>>()?;
        if !payload.schema.registered_in(&object.storage) {
            return Err(invalid());
        }
        let revision = object
            .revision
            .checked_add(1)
            .ok_or_else(|| RuntimeError::module_validation("native revision exhausted"))?;
        let Some(object) = self.native_mut(&mut objects, id) else {
            return Err(invalid());
        };
        object.revision = revision;
        let payload = object.payload_mut::<Managed<T>>()?;
        let _borrow = StorageBorrow::enter(&self.storage_borrows);
        commit(payload)
    }

    pub(crate) fn edit_managed_data<T: NativeData, R>(
        &self,
        id: GcRef,
        edit: impl for<'data> FnOnce(&'data mut T) -> NativeResult<R>,
    ) -> NativeResult<R> {
        self.with_managed_mut(id, |payload: &mut Managed<T>| edit(&mut payload.data))
    }

    pub(crate) fn set_managed_field<T: NativeData>(
        &self,
        id: GcRef,
        schema: &AppliedSchema,
        slot: usize,
        value: Value,
    ) -> NativeResult<()> {
        self.ensure_no_storage_borrow()?;
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
        self.publish_value(value)?;
        self.with_managed_mut(id, |payload: &mut Managed<T>| {
            payload.values[slot] = StoredValue::new(value);
            Ok(())
        })
    }

    pub(crate) fn replace_managed<T: NativeData>(
        &self,
        id: GcRef,
        replacement: Managed<T>,
    ) -> NativeResult<()> {
        self.ensure_no_storage_borrow()?;
        self.ensure_execution_allowed()?;
        if replacement.values.len() != replacement.schema.fields.len() {
            return Err(invalid());
        }
        for (field, value) in replacement.schema.fields.iter().zip(&replacement.values) {
            if !value.value_ref().is_default_heap_payload(self)
                || !field.contract.accepts_value(self, value.value_ref())
            {
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
        self.ensure_structure_mutable(id)?;
        for value in &replacement.values {
            self.publish_value(value.value())?;
        }
        let mut objects = self.objects_mut()?;
        let object = self.native_mut(&mut objects, id).ok_or_else(invalid)?;
        let revision = object.revision.checked_add(1).ok_or_else(invalid)?;
        let charge = replacement.charge();
        let change = self
            .resources
            .prepare_heap_change(object.charge, charge.logical_units)?;
        // Same field count need not mean the same allocation capacity.
        *object.payload_mut::<Managed<T>>()? = replacement;
        object.revision = revision;
        object.charge = charge;
        change.commit(charge);
        Ok(())
    }
}
