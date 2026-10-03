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
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::types::{GenericParam, Ty};
use std::{borrow::Cow, rc::Rc, slice};

impl ExecutionFrame {
    pub fn type_arguments(
        &self,
        runtime: &Runtime,
        types: &[Ty<DefinitionId>],
    ) -> Result<Vec<TypeArgument>, RuntimeError> {
        runtime.type_arguments(self.loaded(), self.environment(), types)
    }

    fn element_contract(
        &self,
        runtime: &Runtime,
        element: &Ty<DefinitionId>,
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
    ) -> Result<Option<Rc<TypeEnvironment>>, RuntimeError> {
        if arguments.iter().all(Ty::is_concrete) {
            return Ok(None);
        }
        let arguments = runtime.type_arguments(self.loaded(), self.environment(), arguments)?;
        if !arguments.iter().any(|argument| argument.has_origin()) {
            return Ok(None);
        }
        let parameters = (0..arguments.len())
            .map(|position| GenericParam {
                owner: *declaration,
                position,
            })
            .collect();
        TypeEnvironment::new(runtime.definition_context(), parameters, arguments)
            .map(|environment| Some(Rc::new(environment)))
    }

    pub fn struct_layout(
        &self,
        runtime: &Runtime,
        id: StructId,
        arguments: &[Ty<DefinitionId>],
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
        arguments: &[Ty<DefinitionId>],
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
