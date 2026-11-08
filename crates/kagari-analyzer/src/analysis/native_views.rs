//! Native presentation identity follows captured physical origins, including aliases.

use crate::{
    analysis::{AnalysisFailure, AnalysisState},
    documents::DocumentInput,
    workspace::{ContextCapture, WorkspaceCapture},
};
use kagari_source::publication::SourceDocument;

impl AnalysisState {
    /// Installed presentation names identify native views directly. Alias names
    /// resolve through the acquisition-owned physical identity captured before
    /// any root read, even when unavailable/conflicting text has no semantic binding.
    pub fn native_document_for(
        &self,
        capture: &WorkspaceCapture,
        name: &str,
    ) -> Option<&SourceDocument> {
        self.native_document(name)
            .or_else(|| self.native_document(capture.document_origins.get(name)?))
    }

    pub fn native_document(&self, name: &str) -> Option<&SourceDocument> {
        self.environment
            .as_ref()?
            .publications
            .iter()
            .find(|document| document.source_name() == name)
    }

    pub(super) fn native_context_document(
        &self,
        context: &ContextCapture,
    ) -> Option<&SourceDocument> {
        let source = context.sources.module(&context.root)?;
        self.native_document(source.name()).or_else(|| {
            let name = context.origins.get(&source.id())?.normalized_key().ok()?;
            self.native_document(&name)
        })
    }

    /// A view never replaces installed contracts. Every open alias must retain
    /// exact presentation bytes; one modified or unavailable alias suppresses
    /// dependent semantic queries until restored or closed.
    pub fn check_native_views(
        &self,
        capture: &WorkspaceCapture,
        documents: &[DocumentInput],
    ) -> Result<(), AnalysisFailure> {
        for document in documents {
            if let Some(expected) = self.native_document_for(capture, &document.source_name)
                && document
                    .source
                    .as_ref()
                    .is_none_or(|source| source.text() != expected.text())
            {
                return Err(AnalysisFailure::Unavailable(format!(
                    "native declaration view {} differs from its installed declarations; restore or close the view",
                    document.uri.as_str()
                )));
            }
        }
        Ok(())
    }
}
