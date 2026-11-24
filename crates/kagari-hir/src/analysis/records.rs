//! Owned semantic facts for the declaration and signature stages.

use crate::{
    AnalysisResult, declarations::Declarations, resolver::resolved::DeclarationNames,
    typeck::ModuleSignatures,
};
use std::sync::Arc;

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
}
