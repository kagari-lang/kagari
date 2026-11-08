//! Completion query policy and protocol presentation.
use crate::analysis::{
    AnalysisFailure, AnalysisState,
    diagnostics::protocol_range,
    features::{FeatureInput, FeatureOutput, FeatureQuery, QueryStep, projection::conversion},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::aggregates::implementations::ImplementationSearchError;
use lsp_types::{CompletionItem, CompletionItemKind, CompletionList, CompletionTextEdit, TextEdit};
use serde_json::to_value;

pub(crate) struct CompletionQuery;

impl FeatureQuery for CompletionQuery {
    const METHOD: &'static str = "textDocument/completion";

    fn run(
        state: &mut AnalysisState,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<QueryStep, AnalysisFailure> {
        state.completion(input, cancel).map(QueryStep::Complete)
    }
}

impl AnalysisState {
    pub fn completion(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<FeatureOutput, AnalysisFailure> {
        let retained = self.editor_for_input(input, cancel)?;
        let editor = &retained.editor;
        let completions = editor
            .completions(input.offset, 4096, cancel)
            .map_err(|error| match error {
                ImplementationSearchError::Cancelled => AnalysisFailure::Cancelled,
                ImplementationSearchError::LimitExceeded => {
                    AnalysisFailure::Capacity("completion search limit exceeded".into())
                }
            })?;
        let range = protocol_range(editor.source(), completions.replacement).ok_or_else(|| {
            AnalysisFailure::Unavailable("completion replacement is outside retained text".into())
        })?;
        let mut items = Vec::new();
        let mut bytes = 0usize;
        for candidate in completions.candidates {
            cancel.check()?;
            let detail = candidate
                .ty
                .as_ref()
                .map(|ty| ty.display_name(editor.definitions()));
            bytes = bytes.saturating_add(
                candidate.name.len() * 3 + detail.as_ref().map_or(0, String::len) + 1024,
            );
            if bytes > input.limit / 2 {
                return Err(AnalysisFailure::Capacity(
                    "completion candidates exceed their byte limit".into(),
                ));
            }
            items.push(CompletionItem {
                label: candidate.name.clone(),
                kind: Some(CompletionItemKind::TEXT),
                detail,
                text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                    range,
                    new_text: candidate.name,
                })),
                ..Default::default()
            });
        }
        FeatureOutput::new(
            to_value(CompletionList {
                is_incomplete: false,
                items,
            })
            .map_err(conversion)?,
            input.limit,
        )
    }
}
