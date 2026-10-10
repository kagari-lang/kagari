//! Resolve aggregate operands with the active frame's checked type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{ExecutionFrame, types::arguments::TypeArgument},
    gc::HeapObjectId,
    module::{EnumVariantRef, StructLayoutRef, layout_scope::LayoutScope},
    native::storage_type::StorageType,
    value::Value,
};
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::table::DefinitionId;
use kagari_types::ty::Ty;
use std::{borrow::Cow, slice, sync::Arc};

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

    fn layout_arguments<'a>(
        &self,
        arguments: &'a [Ty<DefinitionId>],
    ) -> Result<Cow<'a, [Ty<DefinitionId>]>, RuntimeError> {
        if arguments.iter().all(Ty::is_concrete) {
            return Ok(Cow::Borrowed(arguments));
        }
        arguments
            .iter()
            .map(|ty| self.resolve_type(ty).map(Cow::into_owned))
            .collect::<Result<Vec<_>, _>>()
            .map(Cow::Owned)
    }

    fn layout_environment(
        &self,
        runtime: &Runtime,
        declaration: &DefinitionId,
        arguments: &[Ty<DefinitionId>],
    ) -> Result<Option<Arc<LayoutScope>>, RuntimeError> {
        if arguments.iter().all(Ty::is_concrete) {
            return Ok(None);
        }
        let arguments = runtime.type_arguments(
            self.loaded(),
            self.environment()
                .map(|environment| environment.types.clone()),
            arguments,
        )?;
        runtime.prepare_layout_scope(self.loaded(), *declaration, &arguments)
    }

    pub fn struct_layout(
        &self,
        runtime: &Runtime,
        id: StructId,
        arguments: &[Ty<DefinitionId>],
    ) -> Result<StructLayoutRef, RuntimeError> {
        let template = self
            .loaded()
            .bytecode
            .structures
            .get(id.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid struct layout application"))?;
        let scope = self.layout_environment(runtime, &template.declaration, arguments)?;
        runtime
            .modules
            .applied_struct_layout(self.loaded(), id, &self.layout_arguments(arguments)?, scope)
            .ok_or_else(|| RuntimeError::module_validation("invalid struct layout application"))
    }

    pub fn enum_variant(
        &self,
        runtime: &Runtime,
        id: EnumId,
        arguments: &[Ty<DefinitionId>],
        variant: u32,
    ) -> Result<EnumVariantRef, RuntimeError> {
        let template = self
            .loaded()
            .bytecode
            .enumerations
            .get(id.index())
            .ok_or_else(|| RuntimeError::module_validation("invalid enum layout application"))?;
        let scope = self.layout_environment(runtime, &template.declaration, arguments)?;
        runtime
            .modules
            .applied_enum_variant(
                self.loaded(),
                id,
                &self.layout_arguments(arguments)?,
                variant,
                scope,
            )
            .ok_or_else(|| RuntimeError::module_validation("invalid enum layout application"))
    }
}
