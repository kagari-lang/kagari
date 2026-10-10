//! Resolve aggregate operands with the active frame's checked type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, types::arguments::TypeArgument},
    gc::HeapObjectId,
    module::{EnumVariantRef, StructLayoutRef, linked_execution::layouts::AggregateLayout},
    native::storage_type::StorageType,
    value::Value,
};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{slice, sync::Arc};

impl ExecutionFrame {
    pub fn type_arguments(
        &self,
        runtime: &Runtime,
        types: &[Ty<DefinitionId>],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        runtime.type_arguments(
            self.loaded(),
            self.environment()
                .map(|environment| environment.types.clone()),
            types,
        )
    }

    fn element_contract(
        &self,
        runtime: &Runtime,
        element: &Ty<DefinitionId>,
    ) -> Result<Arc<StorageType>, RuntimeError> {
        let argument = runtime
            .type_arguments(
                self.loaded(),
                self.environment()
                    .map(|environment| environment.types.clone()),
                slice::from_ref(element),
            )?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("array element scope"))?;
        StorageType::prepare_scoped(argument, self.loaded()).map(Arc::new)
    }

    pub fn alloc_array(
        &self,
        runtime: &Runtime,
        element: &Ty<DefinitionId>,
        elements: Vec<Value>,
    ) -> Result<HeapObjectId, RuntimeError> {
        runtime.validate_heap_payloads(&elements)?;
        runtime
            .gc
            .alloc_array_with_contract(self.element_contract(runtime, element)?, elements)
    }

    pub fn alloc_array_repeat(
        &self,
        runtime: &Runtime,
        element: &Ty<DefinitionId>,
        value: Value,
        count: usize,
    ) -> Result<HeapObjectId, RuntimeError> {
        runtime.validate_heap_payloads(slice::from_ref(&value))?;
        runtime.gc.alloc_array_repeat_with_contract(
            self.element_contract(runtime, element)?,
            value,
            count,
        )
    }

    fn aggregate_layout(
        &self,
        runtime: &Runtime,
        pc: usize,
    ) -> Result<&AggregateLayout, RuntimeError> {
        self.validate_runtime(runtime)?;
        let links = self
            .links
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("missing function execution"))?;
        let layouts = links
            .layouts
            .as_deref()
            .ok_or_else(|| RuntimeError::module_validation("missing function layout operands"))?;
        layouts.resolve(
            runtime,
            self.loaded(),
            self.environment.as_ref(),
            links.applied_layouts.as_deref(),
            pc,
        )
    }

    pub(super) fn ready_field_layout(&self, pc: usize) -> Option<&StructLayoutRef> {
        let links = self.links.as_ref()?;
        let AggregateLayout::Struct(layout) = links
            .layouts
            .as_ref()?
            .ready(links.applied_layouts.as_deref(), pc)?
        else {
            return None;
        };
        Some(layout)
    }

    pub(super) fn field_layout(
        &self,
        runtime: &Runtime,
        pc: usize,
    ) -> Result<StructLayoutRef, RuntimeError> {
        let AggregateLayout::Struct(layout) = self.aggregate_layout(runtime, pc)? else {
            return Err(RuntimeError::module_validation(
                "expected prepared struct layout",
            ));
        };
        Ok(layout.clone())
    }

    /// Resolve the checked struct operand at this frame's current program point.
    pub fn struct_layout(&self, runtime: &Runtime) -> Result<StructLayoutRef, RuntimeError> {
        self.field_layout(runtime, self.instruction_offset())
    }

    /// Resolve the checked enum operand at this frame's current program point.
    pub fn enum_variant(&self, runtime: &Runtime) -> Result<EnumVariantRef, RuntimeError> {
        let AggregateLayout::Enum(layout) =
            self.aggregate_layout(runtime, self.instruction_offset())?
        else {
            return Err(RuntimeError::module_validation(
                "expected prepared enum layout",
            ));
        };
        Ok(layout.clone())
    }
}
