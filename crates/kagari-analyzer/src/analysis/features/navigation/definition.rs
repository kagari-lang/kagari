//! Definition query policy and protocol presentation.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{
        FeatureInput, FeatureOutput, FeatureQuery, QueryStep,
        projection::{conversion, location, selected_context},
    },
};
use kagari_common::cancellation::CancellationToken;
use serde_json::{Value, to_value};

pub(crate) struct DefinitionQuery;

impl FeatureQuery for DefinitionQuery {
    const METHOD: &'static str = "textDocument/definition";

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure> {
        state.definition(input, cancel).map(QueryStep::Complete)
    }
}

impl AnalysisState {
    pub fn definition(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<FeatureOutput, AnalysisFailure> {
        let retained = self.editor_for_input(input, cancel)?;
        let editor = &retained.editor;
        let value = match editor.definition_at(input.offset) {
            Some(declaration) => {
                let context = selected_context(input)?;
                to_value(location(
                    input,
                    context,
                    editor.signature_snapshot(),
                    declaration.location,
                )?)
                .map_err(conversion)?
            }
            None => Value::Null,
        };
        cancel.check()?;
        FeatureOutput::new(value, input.limit)
    }
}
