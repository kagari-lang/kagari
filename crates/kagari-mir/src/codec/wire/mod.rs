//! MIR file records; semantic and verification ownership remain in the MIR owners.
mod calls;
mod function;
mod instruction;

#[cfg(test)]
mod tests;

use crate::{
    codec::wire::function::MirModuleRecord,
    program::{VerifiedMirProgram, records::MirProgram},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        metadata::DefinitionMetadata,
        scoped::DefinitionReferences,
        table::wire::PortableDefinitionTable,
    },
};
use kagari_contract::{decode_limits, exports::wire::ProgramExportsRecord};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Flattening records and the table preserves the envelope's serialized field order.
/// No semantic model is instantiated with file-local references.
#[derive(Serialize, Deserialize)]
pub(crate) struct ProgramRecord {
    root: ModuleIdentity,
    #[serde(deserialize_with = "decode_limits::modules")]
    modules: Vec<MirModuleRecord>,
    definitions: PortableDefinitionTable,
    exports: ProgramExportsRecord,
}

impl ProgramRecord {
    pub(crate) fn encode(
        program: &VerifiedMirProgram,
        cancel: &CancellationToken,
    ) -> Result<Self, DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut references = HashSet::new();
        for module in program.modules() {
            module.visit_definition_ids(
                &mut |id| {
                    references.insert(*id);
                    Ok(())
                },
                cancel,
            )?;
        }
        program.exports().visit_definition_ids(
            &mut |id| {
                references.insert(*id);
                Ok(())
            },
            cancel,
        )?;
        let encoding = program.definitions().encode(references)?;
        let modules = program
            .modules()
            .iter()
            .map(|module| MirModuleRecord::encode(module, &encoding, cancel))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            root: program.root().clone(),
            modules,
            exports: ProgramExportsRecord::encode(program.exports(), &encoding, cancel)?,
            definitions: encoding.table,
        })
    }

    pub(crate) fn decode(
        self,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<MirProgram>, DefinitionMappingError> {
        check_cancel(cancel)?;
        let decoding = self.definitions.decode()?;
        let modules = self
            .modules
            .into_iter()
            .map(|module| module.decode(&decoding, cancel))
            .collect::<Result<_, _>>()?;
        let exports = self.exports.decode(&decoding, cancel)?;
        DefinitionMetadata::checked(
            decoding.table,
            MirProgram {
                root: self.root,
                modules,
                exports,
            },
            cancel,
        )
    }
}
