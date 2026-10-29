//! Compact-storage kernels for fixed builtin arrays.
//! Indices accept host positions or wide script integers; narrowing failure follows
//! each operation's absence/bounds policy, after its receiver and mutation checks.
use crate::gc::array_storage::{ArrayPayload, ArrayStorage};
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, reference::GcRef},
    native::storage_type::StorageType,
    resource::HeapCharge,
    value::Value,
};
use kagari_types::ty::Ty;
use std::{ops::Bound, sync::Arc};

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid array buffer or payload",
    )
}

impl GcHeap {
    pub(crate) fn buffer_len(&self, id: GcRef) -> Option<usize> {
        self.with_buffer(id, ArrayStorage::len)
    }

    pub(crate) fn buffer_snapshot(&self, id: GcRef) -> Option<Vec<Value>> {
        self.with_buffer(id, ArrayStorage::snapshot)
    }

    pub(crate) fn buffer_get(&self, id: GcRef, index: impl TryInto<usize>) -> Option<Value> {
        let index = index.try_into().ok()?;
        self.with_buffer(id, |values| values.get(index)).flatten()
    }

    /// Distinguish absence from unavailable storage, including a detached edit buffer.
    pub(crate) fn buffer_element(
        &self,
        id: GcRef,
        index: impl TryInto<usize>,
    ) -> Result<Option<Value>, RuntimeError> {
        let index = index.try_into().ok();
        self.with_buffer(id, |values| values.get(index?))
            .ok_or_else(|| RuntimeError::module_validation("array handle length"))
    }

    pub(crate) fn buffer_contract(&self, id: GcRef) -> Option<Arc<StorageType>> {
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
        if !matches!(object.ty, Ty::Array(..)) {
            return None;
        }
        Some(object.payload::<ArrayPayload>().ok()?.contract.clone())
    }

    pub(super) fn validate_buffer_value(
        &self,
        id: GcRef,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        let contract = self.buffer_contract(id).ok_or_else(invalid)?;
        if self.valid_payload(value) && contract.accepts_value(self, value) {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    pub(crate) fn prepare_buffer_values(
        &self,
        contract: &StorageType,
        elements: Vec<Value>,
    ) -> Result<ArrayStorage, RuntimeError> {
        let mut values = ArrayStorage::empty(contract.ty());
        values
            .try_reserve(elements.len())
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        for value in elements {
            if !self.valid_payload(&value) || !contract.accepts_value(self, &value) {
                return Err(invalid());
            }
            values.push(value)?;
        }
        Ok(values)
    }

    pub(crate) fn buffer_fill(&self, id: GcRef, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.validate_buffer_value(id, &value)?;
        let length = self.buffer_len(id).ok_or_else(invalid)?;
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let contract = self.buffer_contract(id).ok_or_else(invalid)?;
        let mut prepared = ArrayStorage::empty(contract.ty());
        prepared
            .try_reserve(length)
            .map_err(|_| self.resource_limit("array fill capacity"))?;
        for start in (0..length).step_by(1024) {
            self.ensure_execution_allowed()?;
            prepared.append_repeated(value, (length - start).min(1024))?;
        }
        self.ensure_execution_allowed()?;
        self.publish_value(value)?;
        self.with_buffer_mut(id, 0, |values| *values = prepared)
    }

    pub(crate) fn buffer_copy_from(
        &self,
        target: GcRef,
        source: GcRef,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let length = self.buffer_len(target).ok_or_else(invalid)?;
        if self.buffer_len(source) != Some(length) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "array copy requires equal lengths",
            ));
        }
        let target_contract = self.buffer_contract(target).ok_or_else(invalid)?;
        let source_contract = self.buffer_contract(source).ok_or_else(invalid)?;
        if !source_contract.same_type(&target_contract) {
            return Err(invalid());
        }
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let prepared = self
            .with_buffer(source, |values| values.copy_range(0, length))
            .ok_or_else(invalid)??;
        self.ensure_execution_allowed()?;
        self.publish_buffer(&prepared)?;
        self.with_buffer_mut(target, 0, |values| *values = prepared)
    }

    pub(crate) fn buffer_copy_within(
        &self,
        target: GcRef,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let bounds = || {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "array copy range is out of bounds",
            )
        };
        let length = self.buffer_len(target).ok_or_else(bounds)?;
        let start = match start {
            Bound::Unbounded => 0,
            Bound::Included(n) => n,
            Bound::Excluded(n) => n.checked_add(1).ok_or_else(bounds)?,
        };
        let end = match end {
            Bound::Unbounded => length,
            Bound::Excluded(n) => n,
            Bound::Included(n) => n.checked_add(1).ok_or_else(bounds)?,
        };
        if start > end || end > length || destination > length || end - start > length - destination
        {
            return Err(bounds());
        }
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(end - start)?;
        let prepared = self
            .with_buffer(target, |values| values.copy_range(start, end))
            .ok_or_else(bounds)??;
        self.ensure_execution_allowed()?;
        self.publish_buffer(&prepared)?;
        self.with_buffer_mut(target, 0, |values| values.overwrite(destination, prepared))?
    }

    pub(crate) fn buffer_set(
        &self,
        id: GcRef,
        index: impl TryInto<usize>,
        value: Value,
    ) -> Result<(), RuntimeError> {
        let index = index.try_into().ok();
        self.ensure_execution_allowed()?;
        self.validate_buffer_value(id, &value)?;
        self.publish_value(value)?;
        self.with_buffer_mut(id, 0, |values| {
            let index = index.ok_or_else(|| {
                RuntimeError::new(
                    RuntimeErrorKind::IndexOutOfBounds,
                    "array index is out of bounds",
                )
            })?;
            values.set(index, value)
        })?
    }

    fn publish_buffer(&self, values: &ArrayStorage) -> Result<(), RuntimeError> {
        for index in 0..values.len() {
            self.publish_value(values.get(index).ok_or_else(invalid)?)?;
        }
        Ok(())
    }

    pub(crate) fn with_buffer<R>(
        &self,
        id: GcRef,
        f: impl FnOnce(&ArrayStorage) -> R,
    ) -> Option<R> {
        self.check_native_operation(id, None, false).ok()?;
        let objects = self.objects.borrow();
        let object = self.native_ref(&objects, id)?;
        if !matches!(object.ty, Ty::Array(..)) {
            return None;
        }
        let array = object.payload::<ArrayPayload>().ok()?;
        Some(f(&array.values))
    }

    pub(super) fn with_buffer_mut<R>(
        &self,
        id: GcRef,
        growth: usize,
        f: impl FnOnce(&mut ArrayStorage) -> R,
    ) -> Result<R, RuntimeError> {
        self.check_native_operation(id, None, true)?;
        let mut objects = self.objects_mut()?;
        let object = self.native_mut(&mut objects, id).ok_or_else(invalid)?;
        let revision = object.revision.checked_add(1).ok_or_else(invalid)?;
        if !matches!(object.ty, Ty::Array(..)) {
            return Err(invalid());
        }
        let max_units = object
            .charge
            .logical_units
            .checked_add(growth)
            .ok_or_else(|| self.resource_limit("array length"))?;
        let change = self
            .resources
            .prepare_heap_change(object.charge, max_units)?;
        let array = object.payload_mut::<ArrayPayload>()?;
        let values = &mut array.values;
        let old_len = values.len();
        let result = f(values);
        let changed = values.len() != old_len;
        let charge = HeapCharge::new(values.len(), values.retained_bytes());

        if changed {
            object.revision = revision;
        }
        object.charge = charge;
        change.commit(charge);
        Ok(result)
    }
}
