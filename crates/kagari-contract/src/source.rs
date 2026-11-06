//! Exact authored text and executable associations, independent of frontend and debug data.
//!
//! These editable records are not a verification seal. The bytecode owner validates
//! them against the complete executable inventory before exposing source queries.
//! Declaration IDs belong to the enclosing `DefinitionMetadata`; ranges are diagnostic
//! locations, never installation-scoped authority to update a declaration.

mod limits;
pub mod manifest;
mod mapping;
pub mod wire;

#[cfg(test)]
mod tests;

use crate::{
    decode_limits::{MAX_NESTED_RECORDS, MAX_TABLE_RECORDS},
    types::ConcreteFunctionIdentity,
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{
        ModuleIdentity,
        mapping::{DefinitionMappingError, check_cancel},
        table::DefinitionId,
    },
    span::Span,
};
use serde::{Deserialize, Serialize};

/// Aggregate source text/metadata budget, also enforced before wire conversion.
pub const MAX_SOURCE_BYTES: usize = 64 * 1024 * 1024;

/// Captured physical document. Text preserves UTF-8 bytes, comments and line endings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceDocument {
    /// Exact authored modules sharing this physical document, including inline children.
    /// Each `(module, uri)` is a portable origin key within this source package.
    /// Neither the display URI alone nor a text fingerprint establishes identity.
    #[serde(deserialize_with = "crate::decode_limits::nested")]
    pub modules: Vec<ModuleIdentity>,
    pub uri: String,
    pub text: String,
    /// Domain-separated content consistency fingerprint, not authentication.
    pub fingerprint: u64,
}

impl SourceDocument {
    pub fn new(modules: Vec<ModuleIdentity>, uri: String, text: String) -> Self {
        Self {
            modules,
            fingerprint: text_fingerprint(&text),
            uri,
            text,
        }
    }
}

/// Whether a complete declaration has a script body or only a declared origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuthoredKind {
    Function,
    AsyncFunction,
    Native,
    Required,
    Constant,
    StaticInitializer,
    Declaration,
}

/// Complete authored declaration extent in one physical document.
///
/// For an inline `mod m { fn f() {} }`, `range` uses offsets in the containing
/// document, while `declaration` belongs to module `m`. Named locals have their
/// own records; a generated closure points to its enclosing authored record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthoredDeclaration {
    pub declaration: DefinitionId,
    pub kind: AuthoredKind,
    pub document: u32,
    pub range: Span,
    /// Exact complete-definition bytes, independent of movement within the document.
    pub fingerprint: u64,
}

/// Generated/computation origin, checked against executable body classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExecutableOrigin {
    Named,
    Closure,
    Async,
    Resume,
    Adapter,
    StaticInitializer,
    Native,
    /// Verified compiler protocol helper/bridge with no authored implementation text.
    Intrinsic,
}

/// One exact executable application and its authored enclosing definition.
/// Native imports can have no authored declaration in the captured closure.
/// Intrinsic helpers always have `authored: None`; their target remains inspectable
/// as known generated/native-origin information, never fabricated source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableSource {
    pub target: ConcreteFunctionIdentity,
    pub authored: Option<DefinitionId>,
    /// Exact host binding for Native origins; absent for script and intrinsic bodies.
    pub native_binding: Option<DefinitionId>,
    pub origin: ExecutableOrigin,
}

/// Optional source package tied to canonical executable facts, excluding this package.
/// Publication/installation identity and update provenance belong to Runtime, not files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourcePackage {
    pub executable_fingerprint: u64,
    pub documents: Vec<SourceDocument>,
    pub declarations: Vec<AuthoredDeclaration>,
    pub executables: Vec<ExecutableSource>,
}

impl SourcePackage {
    /// Bound owned data before cloning, mapping, hashing or encoding it.
    /// This checks shape/resources only, not source-to-executable correspondence.
    pub fn validate_limits(
        &self,
        cancel: &CancellationToken,
    ) -> Result<(), DefinitionMappingError> {
        check_cancel(cancel)?;
        let mut records = self
            .documents
            .len()
            .checked_add(self.declarations.len())
            .and_then(|n| n.checked_add(self.executables.len()))
            .ok_or(DefinitionMappingError::LimitExceeded)?;
        if records > MAX_TABLE_RECORDS {
            return Err(DefinitionMappingError::LimitExceeded);
        }
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
            bytes = bytes
                .checked_add(document.uri.len())
                .and_then(|n| n.checked_add(document.text.len()))
                .ok_or(DefinitionMappingError::LimitExceeded)?;
            if bytes > MAX_SOURCE_BYTES {
                return Err(DefinitionMappingError::LimitExceeded);
            }
        }
        let mut nodes = records;
        for executable in &self.executables {
            check_cancel(cancel)?;
            if executable.target.arguments.len() > MAX_NESTED_RECORDS {
                return Err(DefinitionMappingError::LimitExceeded);
            }
            limits::charge_types(&executable.target.arguments, &mut bytes, &mut nodes, cancel)?;
        }
        if bytes > MAX_SOURCE_BYTES {
            return Err(DefinitionMappingError::LimitExceeded);
        }
        Ok(())
    }
}

/// FNV-1a/64 with a source-text domain. A match is consistency, not trusted authorship.
pub fn text_fingerprint(text: &str) -> u64 {
    hash_bytes(
        hash_bytes(0xcbf29ce484222325, b"kagari-exact-source-v1\0"),
        text.as_bytes(),
    )
}

/// The same content fingerprint with cooperative cancellation during large documents.
pub fn checked_text_fingerprint(
    text: &str,
    cancel: &CancellationToken,
) -> Result<u64, DefinitionMappingError> {
    check_cancel(cancel)?;
    let mut hash = hash_bytes(0xcbf29ce484222325, b"kagari-exact-source-v1\0");
    for chunk in text.as_bytes().chunks(16 * 1024) {
        check_cancel(cancel)?;
        hash = hash_bytes(hash, chunk);
    }
    Ok(hash)
}

fn hash_bytes(hash: u64, bytes: &[u8]) -> u64 {
    bytes.iter().fold(hash, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
