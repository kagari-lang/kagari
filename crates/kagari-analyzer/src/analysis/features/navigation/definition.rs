//! Definition query policy and protocol presentation.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{
        FeatureInput, FeatureQuery, QueryParams, QueryResult, QueryStep,
        projection::{location, selected_context},
    },
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::{
    GotoDefinitionResponse, ProgressToken, TextDocumentPositionParams, request::GotoDefinition,
};

pub(crate) struct DefinitionQuery;

impl FeatureQuery for DefinitionQuery {
    type Request = GotoDefinition;

    fn position(params: &QueryParams<Self>) -> &TextDocumentPositionParams {
        &params.text_document_position_params
    }

    fn work_done(params: &QueryParams<Self>) -> Option<&ProgressToken> {
        params.work_done_progress_params.work_done_token.as_ref()
    }

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        _params: &QueryParams<Self>,
        cancel: &CancellationToken,
    ) -> Result<QueryStep<QueryResult<Self>>, AnalysisFailure> {
        state.definition(input, cancel).map(QueryStep::Complete)
    }
}

impl AnalysisState {
    pub fn definition(
        &mut self,
        input: &FeatureInput,
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
