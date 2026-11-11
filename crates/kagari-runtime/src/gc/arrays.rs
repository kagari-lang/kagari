pub(crate) mod storage;

use crate::{
    error::{RuntimeError, trap::TrapReason},
    gc::{GcHeap, GcObjectKind, HeapObjectId, arrays::storage::PackedStorage},
    module::ExecutableModule,
    native::{
        storage::{NativePayload, NativeStorage},
        storage_type::StorageType,
    },
    resource::HeapCharge,
    value::Value,
};

use kagari_types::ty::Ty;
use std::{ops::Bound, sync::Arc};

/// Exact builtin array storage, with no nominal collection factory/edit policy.
#[derive(Debug)]
pub(crate) struct ArrayPayload {
    pub(crate) contract: Arc<StorageType>,
    pub(crate) values: PackedStorage,
}

impl NativePayload for ArrayPayload {
    fn trace<'payload>(&'payload self, visit: &mut dyn FnMut(&'payload Value)) {
        for value in self.values.traced() {
            visit(value);
        }
    }

    fn charge(&self) -> Result<HeapCharge, RuntimeError> {
        Ok(HeapCharge::new(
            self.values.len(),
            self.values.retained_bytes(),
        ))
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::from(TrapReason::InvalidArrayTargetOrPayload)
}

impl GcHeap {
    pub(super) fn array_storage(&self) -> &NativeStorage {
        self.array_storage
            .get_or_init(NativeStorage::payload::<ArrayPayload>)
    }

    pub(crate) fn alloc_array(
        &self,
        owner: &ExecutableModule,
        element: Ty,
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
        let values = self.prepare_buffer_values(&contract, elements)?;
        let ty = Ty::Array(Box::new(element.clone()));
        let object = self.array_storage().prepare_payload(
            self,
            &ty,
            ArrayPayload { contract, values },
            &owner,
        )?;
        self.alloc_native(object)
    }

    pub(crate) fn alloc_array_repeat(
        &self,
        owner: &ExecutableModule,
        element: Ty,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.alloc_array_repeat_with_contract(
            Arc::new(StorageType::prepare(element, owner)?),
            value,
            count as u64,
        )
    }

    pub(crate) fn alloc_array_repeat_with_contract(
        &self,
        contract: Arc<StorageType>,
        value: Value,
        count: u64,
    ) -> Result<HeapObjectId, RuntimeError> {
        self.ensure_execution_allowed()?;
        if !self.valid_payload(&value) || !contract.accepts_value(self, &value) {
            return Err(invalid());
        }
        let element = contract.ty.clone();
        let owner = contract.owner.clone();
        self.resources.poll_execution()?;
        let count = usize::try_from(count)
            .map_err(|_| self.resource_limit("array length exceeds platform capacity"))?;
        let units = count
            .checked_add(1)
            .ok_or_else(|| self.resource_limit("array length"))?;
        drop(
            self.resources
                .prepare_heap_change(HeapCharge::default(), units)?,
        );
        let mut values = PackedStorage::empty(&element);
        values
            .try_reserve(count)
            .map_err(|_| self.resource_limit("allocation capacity"))?;
        for start in (0..count).step_by(1024) {
            self.ensure_execution_allowed()?;
            values.append_repeated(value, (count - start).min(1024))?;
        }
        let ty = Ty::Array(Box::new(element.clone()));
        let object = self.array_storage().prepare_payload(
            self,
            &ty,
            ArrayPayload { contract, values },
            &owner,
        )?;
        self.alloc_native(object)
    }
}

impl GcHeap {
    fn ensure_array(&self, id: HeapObjectId) -> Result<(), RuntimeError> {
        if self.object_kind(id) == Some(GcObjectKind::Array) && self.buffer_contract(id).is_some() {
            Ok(())
        } else {
            Err(RuntimeError::from(TrapReason::ExpectedABuiltinArrayObject))
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
