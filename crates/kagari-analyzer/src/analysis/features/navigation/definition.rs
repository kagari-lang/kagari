//! Definition query policy and protocol presentation.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{
        DocumentQueryInput,
        projection::{location, selected_context},
    },
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::GotoDefinitionResponse;

impl AnalysisState {
    pub fn definition(
        &mut self,
        input: &DocumentQueryInput,
        cancel: &CancellationToken,
    ) -> Result<Option<GotoDefinitionResponse>, AnalysisFailure> {
        let retained = self.editor_for_input(input, cancel)?;
        let editor = &retained.editor;
        let value = match editor.definition_at(input.offset) {
            Some(declaration) => {
                let context = selected_context(input)?;
                Some(GotoDefinitionResponse::Scalar(location(
                    input,
                    context,
                    editor.signature_snapshot(),
                    declaration.location,
                )?))
            }
            None => None,
        };
        cancel.check()?;
        Ok(value)
    }
}
