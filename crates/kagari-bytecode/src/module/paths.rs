//! Resolve portable path requirements through their original shared schema owner.
use crate::module::BytecodeModule;
use kagari_common::identity::{mapping::DefinitionMappingError, table::DefinitionTable};
use kagari_types::declaration::path::{HostPathContract, HostPathDeclaration};
use std::collections::HashSet;

impl BytecodeModule {
    /// Resolve all declared path requirements, including unused contracts.
    /// Reject duplicates and incomplete schemas before executable access analysis.
    pub fn path_contracts(
        &self,
        definitions: &DefinitionTable,
    ) -> Result<Vec<(&HostPathDeclaration, HostPathContract)>, DefinitionMappingError> {
        let mut seen = HashSet::new();
        self.path_declarations
            .iter()
            .map(|path| {
                if !seen.insert(path) {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                Ok((path, path.contract(&self.external_types, definitions)?))
            })
            .collect()
    }
}
