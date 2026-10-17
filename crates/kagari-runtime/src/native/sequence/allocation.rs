//! Allocate registered sequence storage using its nominal type and scoped element contract.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    module::LoadedModule,
    native::{binding::NativeResult, sequence::SequencePayload, storage_type::StorageType},
    value::Value,
};
use kagari_types::{declaration::native::NativeStorageLayout, ty::Ty};
use std::sync::Arc;

impl Runtime {
    pub(crate) fn allocate_sequence(
        &self,
        owner: &LoadedModule,
        applied: &TypeArgument,
        elements: Vec<Value>,
    ) -> NativeResult<Value> {
        self.gc().ensure_no_native_borrow()?;
        self.validate_loaded_module(owner)?;
        applied.validate(self)?;
        let Ty::NativeObject(nominal) = applied.ty() else {
            return Err(RuntimeError::module_validation(
                "sequence allocation requires a nominal type",
            ));
        };
        let storage = self
            .native_entries
            .storage
            .get_id(nominal.declaration)
            .ok_or_else(|| RuntimeError::module_validation("sequence storage is not installed"))?;
        let NativeStorageLayout::Sequence { element } = storage.layout() else {
            return Err(RuntimeError::module_validation(
                "nominal type is not sequence storage",
            ));
        };
        let contract = Arc::new(StorageType::prepare_scoped(
            applied.parameter(self, owner, element)?,
            owner,
        )?);
        let values = self.gc().prepare_buffer_values(&contract, elements)?;
        let payload = SequencePayload {
            element: contract.ty.clone(),
            contract,
            values,
            leased_units: None,
        };
        let mut object = storage.prepare_payload(self.gc(), applied.ty(), payload, owner)?;
        object.scope = Some(applied.clone());
        self.gc().alloc_native(object).map(Value::GcHandle)
    }
}
