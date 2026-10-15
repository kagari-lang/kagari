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
        let mut schemas = self.external_types.clone();
        // Temporary legacy producer conversion; the shared path resolver is the
        // sole semantic owner. This bridge retires with HostInterface in HR03.
        schemas.extend(
            self.host_interface
                .path_types(definitions)
                .map_err(|_| DefinitionMappingError::InvalidContract)?,
        );
        let mut seen = HashSet::new();
        self.path_declarations
            .iter()
            .chain(&self.host_interface.paths)
            .map(|path| {
                if !seen.insert(path) {
                    return Err(DefinitionMappingError::InvalidContract);
                }
                Ok((path, path.contract(&schemas, definitions)?))
            })
            .collect()
    }
}
