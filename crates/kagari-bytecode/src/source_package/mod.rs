//! Source-free validation and immutable ownership of optional exact source packages.
//!
//! Packages are diagnostics, not executable leases or compilation inputs. No source
//! parser, filesystem lookup, external fetch, MIR or stepping metadata is required.
mod validation;

#[cfg(test)]
mod tests;

use crate::{
    artifact::ArtifactFingerprint,
    program::{
        BytecodeProgram,
        verified::{VerifiedBytecodeProgram, VerifiedProductIdentity},
    },
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        mapping::{DefinitionMapper, DefinitionMappingError},
        metadata::DefinitionMetadata,
        scoped::{DefinitionReferences, RemapDefinitions},
        table::DefinitionTable,
    },
};
use kagari_contract::source::{ExecutableSource, SourcePackage};
use std::sync::Arc;

/// Immutable source records checked against one exact canonical executable product.
/// Cloning this value retains text and identity metadata, never executable authority.
#[derive(Debug, Clone)]
pub struct VerifiedSourcePackage {
    product_identity: VerifiedProductIdentity,
    metadata: Arc<DefinitionMetadata<SourcePackage>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourcePackageError {
    #[error("source package identity or resource error: {0}")]
    Identity(#[from] DefinitionMappingError),
    #[error("source package does not match the executable product")]
    ExecutableMismatch,
    #[error("executable product has no compiler-captured source manifest")]
    MissingManifest,
    #[error("invalid source document identity, content or UTF-8 definition extent")]
    InvalidDocument,
    #[error("source declaration inventory differs from checked executable evidence")]
    DeclarationMismatch,
    #[error("source association differs from checked executable origin evidence")]
    OriginMismatch,
}

impl VerifiedSourcePackage {
    /// Import identities into the supplied bytecode scope and validate all associations.
    /// Hashes establish consistency, not trusted authorship of arbitrary input text.
    pub fn validate(
        package: DefinitionMetadata<SourcePackage>,
        program: &VerifiedBytecodeProgram,
        cancel: &CancellationToken,
    ) -> Result<Self, SourcePackageError> {
        package.records().validate_limits(cancel)?;
        let definitions = program.definitions();
        let records = package
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    let path = package.definitions().resolve(*id)?.to_path();
                    definitions
                        .lookup(&path)
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                definitions,
                cancel,
            ))?;
        if records.executable_fingerprint != program.product_identity().fingerprint().0 {
            return Err(SourcePackageError::ExecutableMismatch);
        }
        validation::validate(&records, program.program(), definitions, cancel)?;
        Ok(Self {
            product_identity: program.product_identity(),
            metadata: Arc::new(DefinitionMetadata::checked(
                definitions.clone(),
                records,
                cancel,
            )?),
        })
    }

    /// Only bytecode-owned envelope validation may transfer already checked records.
    pub(crate) fn from_validated(
        metadata: DefinitionMetadata<SourcePackage>,
        product_identity: VerifiedProductIdentity,
    ) -> Self {
        Self {
            product_identity,
            metadata: Arc::new(metadata),
        }
    }

    /// Attach already verified source to the same canonical verified product.
    /// All references are mapped by exact portable identity into the supplied scope;
    /// missing references reject rather than inventing IDs. No executable is read or
    /// reverified. Cancellation is checked even for same-scope and mismatch paths.
    /// The caller must supply the retained scope of its immutable receiving product.
    pub fn for_product(
        &self,
        product_identity: VerifiedProductIdentity,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, SourcePackageError> {
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        if self.product_identity != product_identity {
            return Err(SourcePackageError::ExecutableMismatch);
        }
        if self.definitions().id() == definitions.id() {
            self.records().visit_definition_ids(
                &mut |id| definitions.resolve(*id).map(|_| ()).map_err(Into::into),
                cancel,
            )?;
            cancel
                .check()
                .map_err(|_| DefinitionMappingError::Cancelled)?;
            return Ok(self.clone());
        }
        let records = self
            .records()
            .remap_definitions(&mut DefinitionMapper::new(
                &mut |id| {
                    let path = self.definitions().resolve(*id)?.to_path();
                    definitions
                        .lookup(&path)
                        .ok_or(DefinitionMappingError::InvalidContract)
                },
                definitions,
                cancel,
            ))?;
        let metadata = DefinitionMetadata::checked(definitions.clone(), records, cancel)?;
        cancel
            .check()
            .map_err(|_| DefinitionMappingError::Cancelled)?;
        Ok(Self::from_validated(metadata, product_identity))
    }

    pub fn records(&self) -> &SourcePackage {
        self.metadata.records()
    }

    pub fn definitions(&self) -> &DefinitionTable {
        self.metadata.definitions()
    }

    /// Returns editable records and their scope, not a reusable validation seal.
    pub fn to_unverified(&self) -> DefinitionMetadata<SourcePackage> {
        self.metadata.as_ref().clone()
    }
}

/// Construct expected associations from already verified compiler body-owner edges.
/// No display name, debug range or candidate-supplied completeness flag participates.
pub fn executable_sources(
    program: &VerifiedBytecodeProgram,
    cancel: &CancellationToken,
) -> Result<Vec<ExecutableSource>, SourcePackageError> {
    validation::executable_sources(program.program(), cancel)
}

pub(crate) fn validate_records(
    package: &SourcePackage,
    program: &BytecodeProgram,
    definitions: &DefinitionTable,
    cancel: &CancellationToken,
) -> Result<(), SourcePackageError> {
    package.validate_limits(cancel)?;
    if package.executable_fingerprint
        != ArtifactFingerprint::checked_program(program, definitions, cancel)?.0
    {
        return Err(SourcePackageError::ExecutableMismatch);
    }
    validation::validate(package, program, definitions, cancel)
}
