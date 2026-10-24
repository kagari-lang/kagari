//! Compiler-captured declaration anchors carried by canonical executable evidence.
//!
//! These facts bind optional text to a compiled product, not to a trusted author.
//! Replacing both executable evidence and text is outside this consistency guarantee.
use crate::{
    decode_limits::{MAX_NESTED_RECORDS, MAX_TABLE_RECORDS},
    source::{AuthoredDeclaration, MAX_SOURCE_BYTES, SourceDocument},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionTable,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Physical origin and exact content captured before executable verification.
/// Display URI and content alone cannot identify a document across packages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocumentAnchor {
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub modules: Vec<ModuleIdentity>,
    pub uri: String,
    pub byte_length: u64,
    pub fingerprint: u64,
}

impl SourceDocumentAnchor {
    pub fn from_document(document: &SourceDocument) -> Self {
        Self {
            modules: document.modules.clone(),
            uri: document.uri.clone(),
            byte_length: document.text.len() as u64,
            fingerprint: document.fingerprint,
        }
    }
}

/// Complete authored inventory collected once by the compiler, independent of bodies,
/// debug tables and retained text. IDs belong to the enclosing executable scope.
/// An absent manifest means source attachment is unsupported, not an empty inventory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceManifest {
    pub documents: Vec<SourceDocumentAnchor>,
    pub declarations: Vec<AuthoredDeclaration>,
}

impl SourceManifest {
    /// Bound anchors and the text they permit before traversal or copying.
    pub fn validate_limits(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut records = self
            .documents
            .len()
            .checked_add(self.declarations.len())
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        let mut bytes = records
            .checked_mul(64)
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        for document in &self.documents {
            check_cancel(cancel)?;
            records = records
                .checked_add(document.modules.len())
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            if document.modules.len() > MAX_NESTED_RECORDS || records > MAX_TABLE_RECORDS {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            bytes = bytes
                .checked_add(
                    usize::try_from(document.byte_length)
                        .map_err(|_| DefinitionMappingError::LimitExceeded)?,
                )
                .and_then(|n| n.checked_add(document.uri.len()))
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            for module in &document.modules {
                check_cancel(cancel)?;
                if !module.within_path_limit() {
                    return Err(DefinitionMappingError::LimitExceeded);
                }
                bytes = bytes
                    .checked_add(64)
                    .and_then(|n| n.checked_add(module.package.0.len()))
                    .ok_or(DefinitionMappingError::LimitExceeded)?;
                for part in &module.path {
                    bytes = bytes
                        .checked_add(8)
                        .and_then(|n| n.checked_add(part.len()))
                        .ok_or(DefinitionMappingError::LimitExceeded)?;
                }
            }
            if bytes > MAX_SOURCE_BYTES {
                return Err(DefinitionMappingError::LimitExceeded);
            }
        }
        if records > MAX_TABLE_RECORDS || bytes > MAX_SOURCE_BYTES {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        Ok(())
    }

    /// Validate scoped IDs, physical authorities and range bounds without a frontend.
    /// UTF-8 boundaries and actual content are checked when attaching retained text.
    pub fn validate(
        &self,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        self.validate_limits(cancel)?;
        let invalid = DefinitionMappingError::InvalidContract;
        let mut origins = HashSet::new();
        for document in &self.documents {
            check_cancel(cancel)?;
            if document.uri.is_empty() || document.modules.is_empty() {
                return Err(invalid);
            }
            for module in &document.modules {
                check_cancel(cancel)?;
                if !origins.insert((module, &document.uri)) {
                    return Err(invalid);
                }
            }
        }
        let mut declarations = HashSet::new();
        let mut used = HashSet::new();
        for declaration in &self.declarations {
            check_cancel(cancel)?;
            let document = self
                .documents
                .get(declaration.document as usize)
                .ok_or(DefinitionMappingError::InvalidContract)?;
            let view = definitions.resolve(declaration.declaration)?;
            if !declarations.insert(declaration.declaration)
                || !document.modules.contains(view.module())
                || declaration.range.start >= declaration.range.end
                || declaration.range.end as u64 > document.byte_length
            {
                return Err(invalid);
            }
            used.insert((view.module(), &document.uri));
        }
        if origins != used {
            return Err(invalid);
        }
        Ok(())
    }
}
