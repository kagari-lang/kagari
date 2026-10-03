//! Resolve aggregate operands with the active frame's checked type arguments.
use crate::{
    Runtime,
    error::RuntimeError,
    frame::{
        ExecutionFrame,
        types::{TypeEnvironment, arguments::TypeArgument},
    },
    gc::HeapObjectId,
    module::{EnumVariantRef, StructLayoutRef},
    native::storage_type::StorageType,
    value::Value,
};
use kagari_abi::types::{AbiType, GenericParameterAbi};
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::DefinitionPath;
use std::{borrow::Cow, rc::Rc, slice};

impl ExecutionFrame {
    pub fn type_arguments(
        &self,
        runtime: &Runtime,
        types: &[AbiType],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        runtime.type_arguments(self.loaded(), self.environment(), types)
    }

    fn element_contract(
        &self,
        runtime: &Runtime,
        element: &AbiType,
    ) -> Result<Rc<StorageType>, RuntimeError> {
        let argument = runtime
            .type_arguments(self.loaded(), self.environment(), slice::from_ref(element))?
            .pop()
            .ok_or_else(|| RuntimeError::module_validation("array element scope"))?;
        StorageType::prepare_scoped(argument, self.loaded()).map(Rc::new)
    }

    pub fn alloc_array(
        &self,
        runtime: &Runtime,
        element: &AbiType,
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
        element: &AbiType,
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
        arguments: &'a [AbiType],
    ) -> Result<Cow<'a, [AbiType]>, RuntimeError> {
        if arguments.iter().all(AbiType::is_concrete) {
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
        declaration: &DefinitionPath,
        arguments: &[AbiType],
    ) -> Result<Option<Rc<TypeEnvironment>>, RuntimeError> {
        if arguments.iter().all(AbiType::is_concrete) {
            return Ok(None);
        }
        let arguments = runtime.type_arguments(self.loaded(), self.environment(), arguments)?;
        if !arguments.iter().any(|argument| argument.has_origin()) {
            return Ok(None);
        }
        let parameters = (0..arguments.len())
            .map(|position| GenericParameterAbi {
                owner: declaration.clone(),
                position,
            })
            .collect();
        TypeEnvironment::new(parameters, arguments).map(|environment| Some(Rc::new(environment)))
    }

    pub fn struct_layout(
        &self,
        runtime: &Runtime,
        id: StructId,
        arguments: &[AbiType],
    ) -> Result<StructLayoutRef, RuntimeError> {
        let mut layout = self
            .loaded()
            .applied_struct_layout(id, &self.layout_arguments(arguments)?)
            .ok_or_else(|| RuntimeError::module_validation("invalid struct layout application"))?;
        layout.environment =
            self.layout_environment(runtime, &layout.layout().declaration, arguments)?;
        Ok(layout)
    }

    pub fn enum_variant(
        &self,
        runtime: &Runtime,
        id: EnumId,
        arguments: &[AbiType],
        variant: u32,
    ) -> Result<EnumVariantRef, RuntimeError> {
        let mut layout = self
            .loaded()
            .applied_enum_variant(id, &self.layout_arguments(arguments)?, variant)
            .ok_or_else(|| RuntimeError::module_validation("invalid enum layout application"))?;
        layout.environment =
            self.layout_environment(runtime, &layout.layout().declaration, arguments)?;
        Ok(layout)
    }
}
