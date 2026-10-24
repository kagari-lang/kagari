//! Explicit source attachment leaves ordinary artifact constructors source-stripped.
use crate::{
    artifact::{ArtifactTables, ArtifactValidationError, KbcArtifact},
    source_package::{SourcePackageError, VerifiedSourcePackage, validate_records},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::RemapDefinitions,
    },
};

impl From<SourcePackageError> for ArtifactValidationError {
    fn from(error: SourcePackageError) -> Self {
        match error {
            SourcePackageError::Identity(error) => Self::Identity(error),
            error => Self::Source(error),
        }
    }
}

impl KbcArtifact {
    /// Attach a matching verified source package and refresh the bounded envelope.
    ///
    /// This is explicit export policy, independent of debug/MIR options. The returned
    /// editable artifact still requires normal validation when loaded. Foreign scope
    /// IDs are imported by exact definition identity; mismatching products reject.
    pub fn with_source_package(
        artifact: DefinitionMetadata<Self>,
        source: &VerifiedSourcePackage,
        cancel: &CancellationToken,
    ) -> Result<DefinitionMetadata<Self>, ArtifactValidationError> {
        let (definitions, mut artifact) = artifact.into_parts();
        let records = source
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    let path = source.definitions().resolve(*id)?.to_path();
                    definitions
                        .lookup(&path)
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                &definitions,
                cancel,
            ))?;
        validate_records(&records, &artifact.program, &definitions, cancel)?;
        artifact.source = Some(records);
        artifact.tables = ArtifactTables::with_metadata(
            &artifact.program,
            artifact.debug.as_ref(),
            artifact.signatures.as_ref(),
            artifact.portable_mir.as_ref(),
            artifact.source.as_ref(),
        );
        artifact.validate_resource_limits(&definitions, cancel)?;
        artifact.header.content_hash = artifact.compute_content_hash(&definitions)?;
        Ok(DefinitionMetadata::checked(definitions, artifact, cancel)?)
    }
}
