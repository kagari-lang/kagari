//! Protocol presentation uses semantic destinations and source-owned coordinates.
use crate::{
    analysis::{
        AnalysisFailure, AnalysisState,
        diagnostics::protocol_range,
        features::{FeatureInput, FeatureOutput},
    },
    workspace::{ContextCapture, origins::file_uri},
};
use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    aggregates::implementations::ImplementationSearchError, analysis::signatures::SignatureSnapshot,
};
use kagari_source::identity::FileSpan;
use kagari_source::source::SourceFile;
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionList, CompletionTextEdit, Hover, HoverContents,
    LanguageString, Location, MarkedString, TextEdit, Uri,
};
use serde_json::{Error, Value, to_value};

impl AnalysisState {
    pub fn hover(
        &mut self,
        input: &FeatureInput,
        cancel: &CancellationToken,
    ) -> Result<FeatureOutput, AnalysisFailure> {
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
            Value::Null
        } else {
            to_value(Hover {
                contents: HoverContents::Array(contents),
                range: None,
            })
            .map_err(conversion)?
        };
        FeatureOutput::new(value, input.limit)
    }

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

pub(crate) fn selected_context(input: &FeatureInput) -> Result<&ContextCapture, AnalysisFailure> {
    let selected = input
        .capture
        .document_contexts
        .get(&input.source_name)
        .ok_or_else(|| {
            AnalysisFailure::Unavailable("query document has no captured context".into())
        })?;
    input
        .capture
        .contexts
        .iter()
        .find(|context| context.key == selected.context)
        .ok_or_else(|| AnalysisFailure::Unavailable("query context is absent".into()))
}

/// A declaration's FileSpan already names its authoritative physical/copy origin.
/// Original open URI spelling wins when the same provider is the queried document.
pub(crate) fn location(
    input: &FeatureInput,
    context: &ContextCapture,
    signatures: &SignatureSnapshot,
    span: FileSpan,
) -> Result<Location, AnalysisFailure> {
    let source = signatures
        .source(span.file)
        .ok_or_else(|| AnalysisFailure::Unavailable("navigation source is not retained".into()))?;
    if source.revision() != span.revision {
        return Err(AnalysisFailure::Unavailable(
            "navigation source revision mismatch".into(),
        ));
    }
    let range = protocol_range(source, span.range).ok_or_else(|| {
        AnalysisFailure::Unavailable("navigation range is outside retained text".into())
    })?;
    let physical = source.origin_id();
    let origin = context.origins.get(&physical);
    let uri = if let Some(uri) = open_destination(
        input,
        origin.map(|origin| origin.provider_key.as_str()),
        source,
    )? {
        uri
    } else if let Some(uri) = origin.and_then(|origin| origin.uri.clone()) {
        uri
    } else {
        file_uri(source.name()).map_err(AnalysisFailure::Unavailable)?
    };
    Ok(Location { uri, range })
}

fn open_destination(
    input: &FeatureInput,
    provider: Option<&str>,
    source: &SourceFile,
) -> Result<Option<Uri>, AnalysisFailure> {
    let mut alias = None;
    for document in &*input.documents {
        let same_provider = provider.is_some_and(|provider| {
            input
                .capture
                .document_contexts
                .get(&document.source_name)
                .and_then(|selected| {
                    input
                        .capture
                        .contexts
                        .iter()
                        .find(|context| context.key == selected.context)
                        .and_then(|context| context.origins.get(&selected.file))
                })
                .is_some_and(|origin| origin.provider_key == provider)
        });
        let same_materialized_view = input
            .capture
            .document_origins
            .get(&document.source_name)
            .is_some_and(|canonical| canonical == source.name());
        if !same_provider && !same_materialized_view && document.source_name != source.name() {
            continue;
        }
        if document
            .source
            .as_ref()
            .is_none_or(|text| text.text() != source.text())
        {
            return Err(AnalysisFailure::Unavailable(
                "navigation destination differs from its admitted open view".into(),
            ));
        }
        if document.uri == input.uri || document.source_name == input.source_name {
            return Ok(Some(document.uri.clone()));
        }
        if alias.is_none() {
            alias = Some(document.uri.clone());
        }
    }
    Ok(alias)
}

fn conversion(error: Error) -> AnalysisFailure {
    AnalysisFailure::Unavailable(format!("semantic result conversion failed: {error}"))
}
