//! Contextual identity traversal of the owning metadata records.
use crate::artifact::{FunctionLayoutMetadata, KbcArtifact, VerificationMetadata};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{
            DefinitionMapper, DefinitionMappingError, DefinitionRecord, check_cancel, map_sequence,
        },
        reference::DefinitionReference,
    },
};

impl<I: DefinitionReference> DefinitionRecord<I> for KbcArtifact<I> {
    type Rebind<J: DefinitionReference> = KbcArtifact<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(KbcArtifact {
            header: self.header.clone(),
            program: self.program.map_identities(mapper)?,
            tables: self.tables.clone(),
            verification: self.verification.map_identities(mapper)?,
            debug: self.debug.clone(),
            signatures: self.signatures.clone(),
            portable_mir: self.portable_mir.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.program.visit_definitions(visit, cancel)?;
        self.verification.visit_definitions(visit, cancel)?;
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for VerificationMetadata<I> {
    type Rebind<J: DefinitionReference> = VerificationMetadata<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(VerificationMetadata {
            bytecode_verified: self.bytecode_verified,
            function_layouts: map_sequence(&self.function_layouts, |value| {
                (value).map_identities(mapper)
            })?,
            function_effects: self.function_effects.clone(),
            control_flow_targets: self.control_flow_targets.clone(),
            typed_path_fingerprints: self.typed_path_fingerprints.clone(),
            public_abi_fingerprints: self.public_abi_fingerprints.clone(),
            dependency_fingerprints: self.dependency_fingerprints.clone(),
            host_interface_fingerprint: self.host_interface_fingerprint,
            loader: self.loader.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        for value0 in &self.function_layouts {
            (value0).visit_definitions(visit, cancel)?;
        }
        Ok(())
    }
}

impl<I: DefinitionReference> DefinitionRecord<I> for FunctionLayoutMetadata<I> {
    type Rebind<J: DefinitionReference> = FunctionLayoutMetadata<J>;

    fn map_identities<J: DefinitionReference>(
        &self,
        mapper: &mut DefinitionMapper<'_, I, J>,
    ) -> Result<Self::Rebind<J>, DefinitionMappingError> {
        mapper.check()?;
        Ok(FunctionLayoutMetadata {
            semantic: self.semantic.map_identities(mapper)?,
            function: self.function,
            params: self.params.clone(),
            return_type: self.return_type,
            locals: self.locals.clone(),
            registers: self.registers.clone(),
            roots: self.roots.clone(),
        })
    }

    fn visit_definitions(
        &self,
        visit: &mut impl FnMut(&I) -> Result<(), DefinitionMappingError>,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        self.semantic.visit_definitions(visit, cancel)?;
        Ok(())
    }
}
