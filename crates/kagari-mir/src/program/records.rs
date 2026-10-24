//! Editable whole-Program records retain root and exports through transformations.

use crate::function::MirModule;
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};
use kagari_contract::{exports::ProgramExports, updates::UpdateEvidence};

/// MIR facts without verification evidence, in the enclosing metadata's identity scope.
#[derive(Debug, Clone)]
pub struct MirProgram {
    pub root: ModuleIdentity,
    pub modules: Vec<MirModule>,
    pub exports: ProgramExports,
    pub update_evidence: UpdateEvidence,
}

impl DefinitionReferences for MirProgram {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for module in &self.modules {
            module.visit_definition_ids(visit, cancel)?;
        }
        self.exports.visit_definition_ids(visit, cancel)?;
        self.update_evidence.visit_definition_ids(visit, cancel)
    }
}

impl RemapDefinitions for MirProgram {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(Self {
            root: self.root.clone(),
            modules: self
                .modules
                .iter()
                .map(|module| module.remap_definitions(mapper))
                .collect::<Result<_, _>>()?,
            exports: self.exports.remap_definitions(mapper)?,
            update_evidence: self.update_evidence.remap_definitions(mapper)?,
        })
    }
}
