//! Implementation query entry; index traversal and cursor lifetime stay with the index owner.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{FeatureInput, FeatureQuery, QueryStep},
};
use kagari_common::cancellation::CancellationToken;

pub(crate) struct ImplementationQuery;

impl FeatureQuery for ImplementationQuery {
    const METHOD: &'static str = "textDocument/implementation";
    const WORK_DONE: bool = true;

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure> {
        state.implementation(input, cancel)
    }
}
