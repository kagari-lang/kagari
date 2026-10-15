//! Contextual identity traversal of the owning metadata records.
use crate::artifact::{FunctionLayoutMetadata, KbcArtifact, VerificationMetadata};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError, check_cancel, map_sequence},
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionId,
    },
};

impl RemapDefinitions for KbcArtifact {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(KbcArtifact {
            header: self.header.clone(),
            program: self.program.remap_definitions(mapper)?,
            tables: self.tables.clone(),
            verification: self.verification.remap_definitions(mapper)?,
            debug: self.debug.clone(),
            signatures: self.signatures.clone(),
            portable_mir: self.portable_mir.clone(),
        })
    }
}

impl DefinitionReferences for KbcArtifact {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.program.visit_definition_ids(visit, cancel)?;
        self.verification.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}

impl RemapDefinitions for VerificationMetadata {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(VerificationMetadata {
            bytecode_verified: self.bytecode_verified,
            function_layouts: map_sequence(&self.function_layouts, |value| {
                (value).remap_definitions(mapper)
            })?,
            function_effects: self.function_effects.clone(),
            control_flow_targets: self.control_flow_targets.clone(),
            typed_path_fingerprints: self.typed_path_fingerprints.clone(),
            public_abi_fingerprints: self.public_abi_fingerprints.clone(),
            dependency_fingerprints: self.dependency_fingerprints.clone(),
            loader: self.loader.clone(),
        })
    }
}

impl DefinitionReferences for VerificationMetadata {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.function_layouts {
            (value0).visit_definition_ids(visit, cancel)?;
        }
        Ok(())
    }
}

impl RemapDefinitions for FunctionLayoutMetadata {
    fn remap_definitions(
        &self,
        mapper: &mut DefinitionMapper<'_>,
    ) -> Result<Self, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionLayoutMetadata {
            semantic: self.semantic.remap_definitions(mapper)?,
            function: self.function,
            params: self.params.clone(),
            return_type: self.return_type,
            locals: self.locals.clone(),
            registers: self.registers.clone(),
            roots: self.roots.clone(),
        })
    }
}

impl DefinitionReferences for FunctionLayoutMetadata {
    fn visit_definition_ids(
        &self,
        visit: &mut impl FnMut(&DefinitionId) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definition_ids(visit, cancel)?;
        Ok(())
    }
}
