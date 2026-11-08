//! Implementation query entry; index traversal and cursor lifetime stay with the index owner.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{FeatureInput, FeatureQuery, QueryParams, QueryResult, QueryStep},
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::{ProgressToken, TextDocumentPositionParams, request::GotoImplementation};

pub(crate) struct ImplementationQuery;

impl FeatureQuery for ImplementationQuery {
    type Request = GotoImplementation;
    const WORK_DONE: bool = true;

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
        state.implementation(input, cancel)
    }
}
