//! Native presentation identity follows captured physical origins, including aliases.

use crate::{
    analysis::{AnalysisFailure, AnalysisState},
    documents::DocumentInput,
    workspace::{ContextCapture, WorkspaceCapture},
};
use kagari_source::generated::GeneratedDocument;

impl AnalysisState {
    /// Installed presentation names identify native views directly. Alias names
    /// resolve through the acquisition-owned physical identity captured before
    /// any root read, even when unavailable/conflicting text has no semantic binding.
    pub fn generated_document_for(
        &self,
        capture: &WorkspaceCapture,
        name: &str,
    ) -> Option<&GeneratedDocument> {
        self.generated_document(name)
            .or_else(|| self.generated_document(capture.document_origins.get(name)?))
    }

    pub fn generated_document(&self, name: &str) -> Option<&GeneratedDocument> {
        self.environment
            .as_ref()?
            .publications
            .iter()
            .find(|document| document.source_name() == name)
    }

    pub(super) fn generated_context_document(
        &self,
        context: &ContextCapture,
    ) -> Option<&GeneratedDocument> {
        let source = context.sources.module(&context.root)?;
        self.generated_document(source.name()).or_else(|| {
            let name = context.origins.get(&source.id())?.normalized_key().ok()?;
            self.generated_document(&name)
        })
    }

    /// A view never replaces installed contracts. Every open alias must retain
    /// exact presentation bytes; one modified or unavailable alias suppresses
    /// dependent semantic queries until restored or closed.
    pub fn check_generated_views(
        &self,
        capture: &WorkspaceCapture,
        documents: &[DocumentInput],
    ) -> Result<(), AnalysisFailure> {
        for document in documents {
            if let Some(generated) = self.generated_document_for(capture, &document.source_name)
                && document
                    .source
                    .as_ref()
                    .is_none_or(|source| source.text() != generated.text())
            {
                return Err(AnalysisFailure::Unavailable(format!(
                    "generated native view {} differs from its installed declarations; restore or close the view",
                    document.uri.as_str()
                )));
            }
        }
        Ok(())
    }
}
