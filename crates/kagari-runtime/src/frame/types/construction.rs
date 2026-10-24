//! Exact construction capabilities retained with their supplying execution scope.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::{MetadataEdge, application_key::AdapterIdentity},
    frame::types::{
        TypeEnvironment, arguments::TypeArgument, bindings::AssociatedInterface,
        compatibility::TypeIdentity,
    },
    gc::interfaces::InterfaceResultBinding,
    module::{ExecutableModule, ModuleKey},
};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ConstructionKey {
    pub(crate) receiver: Arc<TypeIdentity>,
    pub(crate) interface: Arc<TypeIdentity>,
    pub(crate) selection: Option<AdapterIdentity>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConstructionBinding {
    pub(crate) key: ConstructionKey,
    pub(crate) interface: TypeArgument,
    pub(crate) owner: ExecutableModule,
    pub(crate) selection: Option<InterfaceResultBinding>,
    pub(crate) associated: AssociatedInterface,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(crate) struct ConstructionIdentity {
    programs: Vec<ModuleKey>,
    selections: Vec<ConstructionKey>,
}

/// Programs retain finite checked table applications. Only explicit selections
/// authorize a bound construction; mere program membership never grants one.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConstructionScope {
    programs: Arc<Vec<ExecutableModule>>,
    selections: Arc<Vec<ConstructionBinding>>,
}

impl ConstructionScope {
    pub(crate) fn new(owner: &ExecutableModule) -> Self {
        Self {
            programs: Arc::new(vec![owner.program_root()]),
            selections: Arc::default(),
        }
    }

    pub(crate) fn programs(&self) -> &[ExecutableModule] {
        &self.programs
    }

    pub(crate) fn identity(&self) -> ConstructionIdentity {
        ConstructionIdentity {
            programs: self.programs.iter().map(ExecutableModule::key).collect(),
            selections: self
                .selections
                .iter()
                .map(|binding| binding.key.clone())
                .collect(),
        }
    }

    pub(crate) fn selected(
        &self,
        receiver: &TypeIdentity,
        interface: &TypeIdentity,
    ) -> Option<&ConstructionBinding> {
        self.selections.iter().find(|binding| {
            binding.key.receiver.as_ref() == receiver && binding.key.interface.as_ref() == interface
        })
    }

    pub(crate) fn insert(&mut self, binding: ConstructionBinding) {
        if !self
            .selections
            .iter()
            .any(|existing| existing.key == binding.key)
        {
            Arc::make_mut(&mut self.selections).push(binding);
        }
    }

    pub(crate) fn associated_interfaces(&self) -> impl Iterator<Item = AssociatedInterface> + '_ {
        self.selections
            .iter()
            .map(|binding| binding.associated.clone())
    }

    pub(crate) fn extend(&mut self, other: &Self) {
        for program in other.programs() {
            if !self
                .programs
                .iter()
                .any(|existing| existing.program_identity() == program.program_identity())
            {
                Arc::make_mut(&mut self.programs).push(program.clone());
            }
        }
        for binding in other.selections.iter() {
            self.insert(binding.clone());
        }
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.programs.iter().map(MetadataEdge::Program));
        for binding in self.selections.iter() {
            pending.push(MetadataEdge::Program(&binding.owner));
            if let Some(selection) = &binding.selection {
                pending.push(MetadataEdge::Program(&selection.owner));
                if let Some(environment) = &selection.environment {
                    pending.push(MetadataEdge::Environment(environment.id));
                }
            }
        }
    }
}

impl Runtime {
    pub(crate) fn construction_scope(
        &self,
        owner: &ExecutableModule,
        environment: Option<&TypeEnvironment>,
    ) -> Result<ConstructionScope, RuntimeError> {
        self.validate_executable_module(owner)?;
        let mut scope = ConstructionScope::new(owner);
        if let Some(environment) = environment {
            let environment = self
                .gc
                .environment(environment.id)
                .ok_or_else(|| RuntimeError::module_validation("construction environment"))?;
            scope.extend(&environment.constructions);
        }
        Ok(scope)
    }
}
