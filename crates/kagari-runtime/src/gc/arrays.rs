use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    gc::{GcHeap, HeapObjectId},
    module::LoadedModule,
    native::{
        sequence::{SequencePayload, SequenceStorage},
        storage_type::StorageType,
    },
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::sync::Arc;

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
        let values = self.prepare_buffer_values(&contract, elements)?;
        let ty = Ty::Array(Box::new(element.clone()));
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
        let ty = Ty::Array(Box::new(element.clone()));
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
}
