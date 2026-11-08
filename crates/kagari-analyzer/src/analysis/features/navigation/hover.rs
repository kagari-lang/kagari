//! Hover query policy and protocol presentation.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    features::{FeatureInput, FeatureQuery, QueryParams, QueryResult, QueryStep},
};
use kagari_common::cancellation::CancellationToken;
use lsp_types::{Hover, HoverContents, LanguageString, MarkedString};
use lsp_types::{ProgressToken, TextDocumentPositionParams, request::HoverRequest};

pub(crate) struct HoverQuery;

impl FeatureQuery for HoverQuery {
    type Request = HoverRequest;

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
        state.hover(input, cancel).map(QueryStep::Complete)
    }
}

impl AnalysisState {
    pub fn hover(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<Option<Hover>, AnalysisFailure> {
        let retained = self.editor_for_input(input, cancel)?;
        let editor = &retained.editor;
        let mut contents = Vec::new();
        let documentation = editor.documentation_at(input.offset);
        if let Some(documentation) = &documentation {
            if documentation
                .written_signature
                .len()
                .saturating_add(documentation.documentation.len())
                > input.limit / 4
            {
                return Err(AnalysisFailure::Capacity(
                    "hover documentation exceeds its byte limit".into(),
                ));
            }
            if !documentation.written_signature.is_empty() {
                contents.push(MarkedString::LanguageString(LanguageString {
                    language: "kagari".into(),
                    value: documentation.written_signature.clone(),
                }));
            }
            if !documentation.documentation.is_empty() {
                contents.push(MarkedString::String(documentation.documentation.clone()));
            }
        }
        if let Some(ty) = editor
            .type_at(input.offset)
            .filter(|ty| !ty.is_unresolved())
        {
            let spelling = ty.display_name(editor.definitions());
            let name = editor
                .definition_at(input.offset)
                .map(|declaration| declaration.name.as_str());
            let text = name.map_or_else(|| spelling.clone(), |name| format!("{name}: {spelling}"));
            contents.push(MarkedString::LanguageString(LanguageString {
                language: "kagari".into(),
                value: text,
            }));
        }
        cancel.check()?;
        let value = if contents.is_empty() {
            None
        } else {
            Some(Hover {
                contents: HoverContents::Array(contents),
                range: None,
            })
        };
        Ok(value)
    }
}
