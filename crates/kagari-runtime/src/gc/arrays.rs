use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    module::LoadedModule,
    native::{
        sequence::{SequencePayload, SequenceStorage},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::{collection::CollectionAccess, ty::Ty};
use std::{mem, ops::Bound, sync::Arc};

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ScriptTrap,
        "invalid array target or payload",
    )
}

impl GcHeap {
    pub(crate) fn alloc_array(
        &self,
        owner: &LoadedModule,
        element: Ty<DefinitionId>,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array_with_contract(Arc::new(StorageType::prepare(element, owner)?), elements)
    }

    pub(crate) fn alloc_array_with_contract(
        &self,
        contract: Arc<StorageType>,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let owner = contract.owner.clone();
        let element = contract.ty.clone();
        let values = self.prepare_array_values(&contract, elements)?;
        let ty = Ty::Array(Box::new(element.clone()), CollectionAccess::Mutable);
        let object = self.sequence_storage.prepare_payload(
            self,
            &ty,
            SequencePayload {
                leased_units: None,
                element,
                contract,
                values,
            },
            &owner,
        )?;
        self.alloc_native(object)
    }

    pub(crate) fn alloc_array_repeat(
        &self,
        owner: &LoadedModule,
        element: Ty<DefinitionId>,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array_repeat_with_contract(
            Arc::new(StorageType::prepare(element, owner)?),
            value,
            count,
        )
    }

    pub(crate) fn alloc_array_repeat_with_contract(
        &self,
        contract: Arc<StorageType>,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) || !contract.accepts_value(self, &value) {
            return Err(invalid());
        }
        let element = contract.ty.clone();
        let owner = contract.owner.clone();
        self.resources.poll_execution()?;
        let units = count
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("array length"))?;
        drop(self.resources.prepare_heap_growth(units)?);
        let mut values = SequenceStorage::empty(&element);
        values
            .try_reserve(count)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        for start in (0..count).step_by(1024) {
            self.ensure_execution_allowed()?;
            values.append_repeated(value, (count - start).min(1024))?;
        }
        let ty = Ty::Array(Box::new(element.clone()), CollectionAccess::Mutable);
        let object = self.sequence_storage.prepare_payload(
            self,
            &ty,
            SequencePayload {
                leased_units: None,
                element,
                contract,
                values,
            },
            &owner,
        )?;
        self.alloc_native(object)
    }

    pub(super) fn alloc_array_from(
        &self,
        source: HeapObjectId,
        values: SequenceStorage,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        let contract = self.array_contract(source).ok_or_else(invalid)?;
        if mem::discriminant(&values) != mem::discriminant(&SequenceStorage::empty(&contract.ty)) {
            return Err(invalid());
        }
        let object = {
            let objects = self.objects.borrow();
            let HeapObject::Native(object) =
                self.readable_object(&objects, source).ok_or_else(invalid)?
            else {
                return Err(invalid());
            };
            object.replaced_payload(SequencePayload {
                leased_units: None,
                element: contract.ty.clone(),
                contract,
                values,
            })?
        };
        self.alloc_native(object)
    }

    pub(crate) fn clone_array(&self, source: HeapObjectId) -> Result<HeapObjectId, RuntimeError> {
        let values = self
            .with_array(source, |values| values.copy_range(0, values.len()))
            .ok_or_else(invalid)??;
        self.alloc_array_from(source, values)
    }

    pub fn array_len(&self, id: HeapObjectId) -> Option<usize> {
        self.with_array(id, SequenceStorage::len)
    }

    pub fn array_snapshot(&self, id: HeapObjectId) -> Option<Vec<Value>> {
        self.with_array(id, SequenceStorage::snapshot)
    }

    pub fn array_get(&self, id: HeapObjectId, index: usize) -> Option<Value> {
        self.with_array(id, |values| values.get(index)).flatten()
    }

    pub(crate) fn array_contract(&self, id: HeapObjectId) -> Option<Arc<StorageType>> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Array(..)) {
            return None;
        }
        Some(object.payload::<SequencePayload>().ok()?.contract.clone())
    }

    fn validate_array_value(&self, id: HeapObjectId, value: &Value) -> Result<(), RuntimeError> {
        let contract = self.array_contract(id).ok_or_else(invalid)?;
        if self.valid_payload(value) && contract.accepts_value(self, value) {
            Ok(())
        } else {
            Err(invalid())
        }
    }

    pub(super) fn prepare_array_values(
        &self,
        contract: &StorageType,
        elements: Vec<Value>,
    ) -> Result<SequenceStorage, RuntimeError> {
        let mut values = SequenceStorage::empty(&contract.ty);
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

    pub fn array_push(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        self.validate_array_value(id, &value)?;
        let growth = self.resources.prepare_heap_growth(1)?;
        self.with_array_mut(id, |values| {
            values
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            values.push(value)
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }

    pub fn array_pop(&self, id: HeapObjectId) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, SequenceStorage::pop)
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_insert(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        self.validate_array_value(id, &value)?;
        if index > self.array_len(id).ok_or_else(invalid)? {
            return Err(invalid());
        }
        let growth = self.resources.prepare_heap_growth(1)?;
        self.with_array_mut(id, |values| {
            values
                .try_reserve(1)
                .map_err(|_| self.resource_limit("allocation capacity"))?;
            values.insert(index, value)
        })
        .ok_or_else(invalid)??;
        growth.commit();
        Ok(())
    }

    pub fn array_remove(
        &self,
        id: HeapObjectId,
        index: usize,
    ) -> Result<Option<Value>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let value = self
            .with_array_mut(id, |values| values.remove(index))
            .ok_or_else(invalid)?;
        if value.is_some() {
            self.release_heap_units(1);
        }
        Ok(value)
    }

    pub fn array_clear(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_structure_mutable(id)?;
        let removed = self
            .with_array_mut(id, |values| {
                let length = values.len();
                values.clear();
                length
            })
            .ok_or_else(invalid)?;
        self.release_heap_units(removed);
        Ok(())
    }

    pub fn array_fill(&self, id: HeapObjectId, value: Value) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(id)?;
        self.validate_array_value(id, &value)?;
        let length = self.array_len(id).ok_or_else(invalid)?;
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let contract = self.array_contract(id).ok_or_else(invalid)?;
        let mut prepared = SequenceStorage::empty(&contract.ty);
        prepared
            .try_reserve(length)
            .map_err(|_| self.resource_limit("array fill capacity"))?;
        for start in (0..length).step_by(1024) {
            self.ensure_execution_allowed()?;
            prepared.append_repeated(value, (length - start).min(1024))?;
        }
        self.ensure_execution_allowed()?;
        self.with_array_mut(id, |values| *values = prepared)
            .ok_or_else(invalid)
    }

    pub fn array_copy_from(
        &self,
        target: HeapObjectId,
        source: HeapObjectId,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(target)?;
        let length = self.array_len(target).ok_or_else(invalid)?;
        if self.array_len(source) != Some(length) {
            return Err(RuntimeError::new(
                RuntimeErrorKind::ScriptTrap,
                "array copy requires equal lengths",
            ));
        }
        let target_contract = self.array_contract(target).ok_or_else(invalid)?;
        let source_contract = self.array_contract(source).ok_or_else(invalid)?;
        if !source_contract.same_type(&target_contract) {
            return Err(invalid());
        }
        self.resources.poll_execution()?;
        let _temporary = self.resources.reserve_temporary_heap(length)?;
        let prepared = self
            .with_array(source, |values| values.copy_range(0, length))
            .ok_or_else(invalid)??;
        self.ensure_execution_allowed()?;
        self.with_array_mut(target, |values| *values = prepared)
            .ok_or_else(invalid)
    }

    pub fn array_copy_within(
        &self,
        target: HeapObjectId,
        start: Bound<usize>,
        end: Bound<usize>,
        destination: usize,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(target)?;
        let bounds = || {
            RuntimeError::new(
                RuntimeErrorKind::IndexOutOfBounds,
                "array copy range is out of bounds",
            )
        };
        let length = self.array_len(target).ok_or_else(bounds)?;
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
            .with_array(target, |values| values.copy_range(start, end))
            .ok_or_else(bounds)??;
        self.ensure_execution_allowed()?;
        self.with_array_mut(target, |values| values.overwrite(destination, prepared))
            .ok_or_else(bounds)?
    }

    pub fn array_set(
        &self,
        id: HeapObjectId,
        index: usize,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        self.ensure_callback_mutable(id)?;
        self.validate_array_value(id, &value)?;
        self.with_array_mut(id, |values| values.set(index, value))
            .ok_or_else(invalid)?
    }

    pub(crate) fn with_array<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&SequenceStorage) -> R,
    ) -> Option<R> {
        let objects = self.objects.borrow();
        let HeapObject::Native(object) = self.readable_object(&objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Array(..)) {
            return None;
        }
        let sequence = object.payload::<SequencePayload>().ok()?;
        if sequence.leased_units.is_some() {
            return None;
        }
        Some(f(&sequence.values))
    }

    pub(super) fn with_array_mut<R>(
        &self,
        id: HeapObjectId,
        f: impl FnOnce(&mut SequenceStorage) -> R,
    ) -> Option<R> {
        let mut objects = self.objects_mut().ok()?;
        let revision = objects.get(id.index())?.revision.checked_add(1)?;
        let HeapObject::Native(object) = self.object_mut(&mut objects, id)? else {
            return None;
        };
        if !matches!(object.ty, Ty::Array(..)) {
            return None;
        }
        let values = &mut object.payload_mut::<SequencePayload>().ok()?.values;
        let old_len = values.len();
        let result = f(values);
        if values.len() != old_len {
            objects[id.index()].revision = revision;
        }
        Some(result)
    }
}
