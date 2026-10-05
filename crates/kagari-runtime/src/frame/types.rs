//! Reified type arguments retained by a shared frame or closure.
pub mod arguments;
pub(crate) mod bindings;
pub(crate) mod compatibility;
pub(crate) mod operations;
use crate::{
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        environments::EnvironmentId,
        groups::{OperationGroupId, OperationId},
    },
    frame::types::{
        arguments::TypeArgument, bindings::TypeBindings, operations::OperationBindings,
    },
    gc::GcHeap,
};
use kagari_common::identity::{map::DefinitionContext, table::DefinitionId};
use kagari_types::{
    declaration::requirement::NativeCallableRequirement,
    ty::{GenericParam, NominalTy, Ty},
};
use std::rc::Rc;

#[derive(Debug, Clone)]
pub(crate) struct EnvironmentRecord {
    pub(crate) types: Rc<TypeBindings>,
    parent: Option<EnvironmentId>,
    operations: OperationBindings,
}

impl EnvironmentRecord {
    pub(crate) fn validate(&self, heap: &GcHeap) -> bool {
        self.parent
            .is_none_or(|parent| heap.environment(parent).is_some())
            && self.operations.validate(heap)
    }

    pub(crate) fn trace_metadata<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        if let Some(parent) = &self.parent {
            pending.push(MetadataEdge::Environment(*parent));
        }
        self.operations.trace_metadata(pending);
    }

    pub(crate) fn new(
        definitions: &DefinitionContext,
        parameters: Vec<GenericParam<DefinitionId>>,
        arguments: Vec<TypeArgument>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            types: Rc::new(TypeBindings::new(definitions, parameters, arguments)?),
            parent: None,
            operations: OperationBindings::default(),
        })
    }

    pub(crate) fn include(&mut self, parent: Option<TypeEnvironment>) -> Result<(), RuntimeError> {
        if let Some(parent) = parent {
            Rc::make_mut(&mut self.types).include(Some(parent.types.clone()))?;
            self.parent = Some(parent.id);
        }
        Ok(())
    }

    pub(crate) fn operations(&self) -> &OperationBindings {
        &self.operations
    }

    fn refresh_types(&mut self) {
        Rc::make_mut(&mut self.types).associated_interfaces =
            self.operations.associated_interfaces();
    }
    #[cfg(test)]
    pub(crate) fn add_operation(
        &mut self,
        heap: &GcHeap,
        id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.operations.push(heap, id)?;
        self.refresh_types();
        Ok(())
    }
    pub(crate) fn add_receiver(
        &mut self,
        heap: &GcHeap,
        id: OperationGroupId,
    ) -> Result<(), RuntimeError> {
        self.operations.receiver(heap, id)?;
        self.refresh_types();
        Ok(())
    }
    pub(crate) fn extend_operations(&mut self, operations: OperationBindings) {
        self.operations.extend(operations);
        self.refresh_types();
    }
}

/// A checked executable identity plus immutable type facts. This handle is not a root.
#[derive(Debug, Clone)]
pub struct TypeEnvironment {
    pub(crate) id: EnvironmentId,
    pub(crate) types: Rc<TypeBindings>,
}

impl TypeEnvironment {
    pub(crate) fn operation_slot(
        &self,
        heap: &GcHeap,
        receiver: &Ty<DefinitionId>,
        interface: &NominalTy<DefinitionId>,
        slot: u32,
    ) -> Option<OperationId> {
        heap.environment(self.id)?
            .operations
            .operation_slot(heap, receiver, interface, slot)
    }

    pub(crate) fn operation(
        &self,
        heap: &GcHeap,
        required: &NativeCallableRequirement<DefinitionId>,
    ) -> Option<OperationId> {
        heap.environment(self.id)?
            .operations
            .operation(heap, required)
    }
}
