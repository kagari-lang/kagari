//! Reified type arguments retained by a shared frame or closure.
#[cfg(test)]
use crate::execution_metadata::groups::OperationId;

pub mod arguments;
pub(crate) mod bindings;
pub(crate) mod compatibility;
pub(crate) mod construction;
pub(crate) mod operations;
use crate::{
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge, environments::EnvironmentId, groups::group::OperationGroup,
    },
    frame::types::{
        arguments::TypeArgument, bindings::TypeBindings, construction::ConstructionScope,
        operations::OperationBindings,
    },
};
use kagari_common::identity::table::DefinitionTable;
use kagari_types::ty::GenericParam;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct EnvironmentRecord {
    pub(crate) types: Arc<TypeBindings>,
    pub(crate) constructions: ConstructionScope,
    parent: Option<EnvironmentId>,
    operations: OperationBindings,
}

impl EnvironmentRecord {
    pub(crate) fn trace_metadata<'a, E, F>(&'a self, visit: &mut F) -> Result<(), E>
    where
        F: FnMut(MetadataEdge<'a>) -> Result<(), E> + ?Sized,
    {
        if let Some(parent) = &self.parent {
            visit(MetadataEdge::Environment(*parent))?;
        }
        self.operations.trace_metadata(visit)?;
        self.constructions.trace(visit)
    }

    pub(crate) fn new(
        definitions: &DefinitionTable,
        parameters: Vec<GenericParam>,
        arguments: Vec<TypeArgument>,
    ) -> Result<Self, RuntimeError> {
        Ok(Self {
            types: Arc::new(TypeBindings::new(definitions, parameters, arguments)?),
            parent: None,
            operations: OperationBindings::default(),
            constructions: ConstructionScope::default(),
        })
    }

    pub(crate) fn include(&mut self, parent: Option<TypeEnvironment>) -> Result<(), RuntimeError> {
        if let Some(parent) = parent {
            Arc::make_mut(&mut self.types).include(Some(parent.types.clone()))?;
            self.parent = Some(parent.id);
        }
        Ok(())
    }

    pub(crate) fn operations(&self) -> &OperationBindings {
        &self.operations
    }

    pub(crate) fn set_constructions(&mut self, constructions: ConstructionScope) {
        self.constructions = constructions;
        self.refresh_types();
    }

    fn refresh_types(&mut self) {
        let mut associated = self
            .constructions
            .associated_interfaces()
            .collect::<Vec<_>>();
        associated.extend(self.operations.associated_interfaces().iter().cloned());
        Arc::make_mut(&mut self.types).associated_interfaces = Arc::new(associated);
    }

    #[cfg(test)]
    pub(crate) fn add_operation(
        &mut self,
        group: &OperationGroup,
        id: OperationId,
    ) -> Result<(), RuntimeError> {
        self.operations.push(group, id)?;
        self.refresh_types();
        Ok(())
    }

    pub(crate) fn add_receiver(&mut self, group: &OperationGroup) -> Result<(), RuntimeError> {
        self.operations.receiver(group)?;
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
    pub(crate) types: Arc<TypeBindings>,
}
