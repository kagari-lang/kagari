//! Shared protocol coordinate and authoritative destination projection.
use crate::{
    analysis::{AnalysisFailure, diagnostics::protocol_range, features::FeatureInput},
    workspace::{ContextCapture, origins::file_uri},
};
use kagari_hir::analysis::signatures::SignatureSnapshot;
use kagari_source::{identity::FileSpan, source::SourceFile};
use lsp_types::{Location, Uri};
use serde_json::Error;

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

pub(super) fn conversion(error: Error) -> AnalysisFailure {
    AnalysisFailure::Unavailable(format!("semantic result conversion failed: {error}"))
}
