//! Checked construction applications supplied by lexical execution programs.
use crate::{
    Runtime,
    error::RuntimeError,
    execution_metadata::MetadataEdge,
    frame::types::TypeEnvironment,
    module::{LoadedModule, ModuleKey},
};
use std::sync::Arc;

/// A finite set of retained programs, never a live module-store search. Shared
/// bodies can use applications checked by callers that were loaded after them.
#[derive(Debug, Clone, Default)]
pub(crate) struct ConstructionScope {
    programs: Arc<Vec<LoadedModule>>,
}

impl ConstructionScope {
    pub(crate) fn new(owner: &LoadedModule) -> Self {
        Self {
            programs: Arc::new(vec![owner.program_root()]),
        }
    }

    pub(crate) fn programs(&self) -> &[LoadedModule] {
        &self.programs
    }

    pub(crate) fn identity(&self) -> Vec<ModuleKey> {
        self.programs.iter().map(LoadedModule::key).collect()
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
    }

    pub(crate) fn trace<'a>(&'a self, pending: &mut Vec<MetadataEdge<'a>>) {
        pending.extend(self.programs.iter().map(MetadataEdge::Program));
    }
}

impl Runtime {
    pub(crate) fn construction_scope(
        &self,
        owner: &LoadedModule,
        environment: Option<&TypeEnvironment>,
    ) -> Result<ConstructionScope, RuntimeError> {
        self.validate_loaded_module(owner)?;
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
