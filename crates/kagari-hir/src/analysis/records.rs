//! Persistent semantic records contain owned facts, never a lowered HIR graph.
//!
//! HIR-local IDs are meaningful only with the exact lowering supplied by a live
//! query. Weak graph/query handles in these caches are transitional for the owned
//! graph: move them to scoped sessions at the borrowed-HIR cutover. They are not
//! an arena lifetime or independent-region reclamation proof.

use crate::{
    AnalysisResult, DiagnosticBuffer,
    analysis::{
        declarations::FileDeclarations, signatures::FileSignatures,
        source_preparation::ParsedModule,
    },
    declarations::Declarations,
    lower::LoweredModule,
    resolver::resolved::DeclarationNames,
    typeck::ModuleSignatures,
};
use kagari_source::{
    identity::{FileId, Revision},
    source::SourceFile,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Weak},
};

/// Declaration-stage identity and name facts, shared with the graph-bearing view.
#[derive(Debug, Clone)]
pub(crate) struct DeclarationRecord {
    pub(crate) names: Arc<AnalysisResult<DeclarationNames>>,
    pub(crate) declarations: Declarations,
}

/// Signature-stage declarations include checked imported types and their exact
/// definition prefix; they are a distinct semantic stage, not the header record.
#[derive(Debug, Clone)]
pub(crate) struct SignatureRecord {
    pub(crate) names: Arc<AnalysisResult<DeclarationNames>>,
    pub(crate) declarations: Declarations,
    pub(crate) signatures: Arc<AnalysisResult<ModuleSignatures>>,
    /// Diagnostics after this boundary are recomputed against completed aggregates.
    pub(crate) local_signature_diagnostics: usize,
}

#[derive(Debug, Clone)]
pub(super) struct DeclarationCacheEntry {
    pub(super) record: Arc<DeclarationRecord>,
    pub(super) source: Arc<SourceFile>,
    pub(super) parsed: ParsedModule,
    pub(super) diagnostics: Arc<DiagnosticBuffer>,
    pub(super) query: Weak<FileDeclarations>,
    pub(super) lowered: Weak<LoweredModule>,
}

#[derive(Debug, Clone)]
pub(super) struct DeclarationCache {
    pub(super) revision: Revision,
    pub(super) files: BTreeMap<FileId, DeclarationCacheEntry>,
}

#[derive(Debug, Clone)]
pub(super) struct SignatureCacheEntry {
    pub(super) record: Arc<SignatureRecord>,
    pub(super) declaration: Arc<DeclarationRecord>,
    pub(super) diagnostics: Arc<DiagnosticBuffer>,
    pub(super) query: Weak<FileSignatures>,
    pub(super) lowered: Weak<LoweredModule>,
    pub(super) signatures_reused: bool,
}

#[derive(Debug, Clone)]
pub(super) struct SignatureCache {
    pub(super) revision: Revision,
    pub(super) files: BTreeMap<FileId, SignatureCacheEntry>,
}
