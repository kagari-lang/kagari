//! Convert retained analysis locations to bounded, owned protocol contributions.
pub(crate) mod group;

use crate::{
    analysis::{AnalysisFailure, AnalysisState, diagnostics::group::DiagnosticGroup},
    documents::DocumentInput,
    protocol::source_name,
    workspace::{CaptureIssue, ContextCapture, ContextKey, WorkspaceCapture, origins::file_uri},
};
use kagari_common::{cancellation::CancellationToken, span::Span};
use kagari_hir::analysis::AnalysisSnapshot;
use kagari_source::{
    diagnostic::{Diagnostic as SourceDiagnostic, Severity},
    line_index::PositionEncoding,
    source::SourceFile,
};
use kagari_syntax::parser::{ParseOptions, parse};
use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Uri};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy)]
pub(crate) struct ProjectionLimits {
    pub uris: usize,
    pub diagnostics: usize,
    pub bytes: usize,
}

#[derive(Debug)]
pub(crate) struct DiagnosticsResult {
    pub groups: Vec<DiagnosticGroup>,
    /// Failures without a trustworthy source location remain visible as messages.
    pub issues: Vec<String>,
}

impl AnalysisState {
    /// Project one context, or the capture's global acquisition issues. The latter
    /// does not require a prepared native environment and remains useful on failure.
    pub fn diagnostics(
        &mut self,
        capture: &WorkspaceCapture,
        key: Option<&ContextKey>,
        documents: &[DocumentInput],
        cancel: &CancellationToken,
    ) -> Result<DiagnosticsResult, AnalysisFailure> {
        cancel.check()?;
        let mut projection = Projection::new(capture, documents, self.projection_limits);
        let native_views_valid = self.check_generated_views(capture, documents);
        match key {
            None => {
                for document in documents {
                    if let Some(generated) =
                        self.generated_document_for(capture, &document.source_name)
                        && document
                            .source
                            .as_ref()
                            .is_none_or(|source| source.text() != generated.text())
                    {
                        projection.modified_native(document);
                    }
                }
                for issue in &capture.errors {
                    cancel.check()?;
                    projection.acquisition(issue, None);
                }
            }
            Some(key) => {
                let context = capture
                    .contexts
                    .iter()
                    .find(|context| &context.key == key)
                    .ok_or_else(|| {
                        AnalysisFailure::Unavailable("diagnostic context is absent".into())
                    })?;
                let generated_view = self.generated_context_document(context).is_some();
                if generated_view {
                    // The environment bootstrap already checked these exact bytes.
                    // Re-parsing a view as script would create a second module owner.
                } else if context.semantic_available
                    && native_views_valid.is_ok()
                    && self.environment.is_some()
                {
                    for issue in &context.errors {
                        cancel.check()?;
                        projection.acquisition(issue, Some(context));
                    }
                    let retained = self.snapshot(context, cancel)?;
                    let snapshot = &retained.snapshot;
                    for declaration in snapshot.declaration_snapshot().files() {
                        cancel.check()?;
                        let source = declaration.source();
                        let file = snapshot.file(source.id()).ok_or_else(|| {
                            AnalysisFailure::Unavailable(
                                "full analysis omitted a declared source".into(),
                            )
                        })?;
                        for diagnostic in file.result().diagnostics() {
                            cancel.check()?;
                            projection.semantic(context, snapshot, source, diagnostic);
                            projection.check()?;
                        }
                    }
                } else {
                    for issue in &context.errors {
                        cancel.check()?;
                        projection.acquisition(issue, Some(context));
                    }
                    if let Err(error) = &native_views_valid {
                        projection.issue(error.to_string());
                    }
                    if self.environment.is_none() {
                        projection.issue("Native environment is unavailable; only syntax diagnostics are available.".into());
                    }
                    for source in context.sources.files() {
                        cancel.check()?;
                        let parsed = parse(source, ParseOptions::default(), cancel)?;
                        for diagnostic in parsed.diagnostics().iter() {
                            projection.syntax(context, source, diagnostic);
                            projection.check()?;
                        }
                    }
                }
                projection.seed(context);
            }
        }
        cancel.check()?;
        projection.finish()
    }
}

struct Projection<'a> {
    capture: &'a WorkspaceCapture,
    documents: &'a [DocumentInput],
    aliases: BTreeMap<String, BTreeSet<Uri>>,
    groups: BTreeMap<Uri, Vec<Diagnostic>>,
    issues: BTreeSet<String>,
    limits: ProjectionLimits,
    records: usize,
    bytes: usize,
    failure: bool,
}

impl<'a> Projection<'a> {
    fn new(
        capture: &'a WorkspaceCapture,
        documents: &'a [DocumentInput],
        limits: ProjectionLimits,
    ) -> Self {
        let mut aliases = BTreeMap::<String, BTreeSet<Uri>>::new();
        for document in documents {
            let Some(binding) = capture.document_contexts.get(&document.source_name) else {
                continue;
            };
            if binding.version != document.version || binding.incarnation != document.incarnation {
                continue;
            }
            let origin = capture
                .contexts
                .iter()
                .find(|context| context.key == binding.context)
                .and_then(|context| context.origins.get(&binding.file));
            if let Some(origin) = origin {
                aliases
                    .entry(origin.provider_key.clone())
                    .or_default()
                    .insert(document.uri.clone());
            }
        }
        Self {
            capture,
            documents,
            aliases,
            groups: BTreeMap::new(),
            issues: BTreeSet::new(),
            limits,
            records: 0,
            bytes: 0,
            failure: false,
        }
    }

    fn check(&self) -> Result<(), AnalysisFailure> {
        if self.failure {
            Err(AnalysisFailure::Capacity(
                "diagnostic projection exceeds its retained-result budget".into(),
            ))
        } else {
            Ok(())
        }
    }

    fn issue(&mut self, message: String) {
        if self.failure || self.issues.contains(&message) {
            return;
        }
        let bytes = message.capacity().saturating_add(128);
        if self.records >= self.limits.diagnostics
            || bytes > self.limits.bytes.saturating_sub(self.bytes)
        {
            self.failure = true;
            return;
        }
        self.records += 1;
        self.bytes += bytes;
        self.issues.insert(message);
    }

    fn push(&mut self, uri: Uri, diagnostic: Diagnostic) {
        if self.failure {
            return;
        }
        // Projected records have only scalar fields and these three strings. The
        // fixed allowance also covers the group/URI, vector growth and map nodes.
        let code = diagnostic.code.as_ref().map_or(0, |code| match code {
            NumberOrString::String(value) => value.capacity(),
            NumberOrString::Number(_) => 0,
        });
        let bytes = 1024usize
            .saturating_add(uri.as_str().len())
            .saturating_add(diagnostic.message.capacity())
            .saturating_add(code)
            .saturating_add(diagnostic.source.as_ref().map_or(0, String::capacity));
        if self.records >= self.limits.diagnostics
            || (!self.groups.contains_key(&uri) && self.groups.len() >= self.limits.uris)
            || bytes > self.limits.bytes.saturating_sub(self.bytes)
        {
            self.failure = true;
            return;
        }
        self.records += 1;
        self.bytes += bytes;
        self.groups.entry(uri).or_default().push(diagnostic);
    }

    fn uris(&self, context: &ContextCapture, source: &SourceFile) -> Vec<Uri> {
        context
            .origins
            .get(&source.origin_id())
            .map(|origin| {
                self.aliases
                    .get(&origin.provider_key)
                    .map(|uris| uris.iter().cloned().collect())
                    .unwrap_or_else(|| origin.uri.iter().cloned().collect::<Vec<_>>())
            })
            .unwrap_or_else(|| file_uri(source.name()).into_iter().collect())
    }

    fn seed(&mut self, context: &ContextCapture) {
        for source in context.sources.files() {
            for uri in self.uris(context, source) {
                if self.failure || self.groups.contains_key(&uri) {
                    continue;
                }
                let bytes = 512usize.saturating_add(uri.as_str().len());
                if self.groups.len() >= self.limits.uris
                    || bytes > self.limits.bytes.saturating_sub(self.bytes)
                {
                    self.failure = true;
                    return;
                }
                self.bytes += bytes;
                self.groups.insert(uri, Vec::new());
            }
        }
    }

    fn modified_native(&mut self, document: &DocumentInput) {
        let message = "Generated native declarations are read-only; restore or close this view to resume dependent semantics.";
        let range = document
            .source
            .as_ref()
            .and_then(|source| protocol_range(source, Span::new(0, source.text().len())));
        if let Some(range) = range {
            self.push(
                document.uri.clone(),
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("KG_NATIVE_VIEW_MODIFIED".into())),
                    source: Some("kagari".into()),
                    message: message.into(),
                    ..Diagnostic::default()
                },
            );
        } else {
            self.issue(format!("{}: {message}", document.uri.as_str()));
        }
    }

    fn semantic(
        &mut self,
        context: &ContextCapture,
        snapshot: &AnalysisSnapshot,
        source: &SourceFile,
        diagnostic: &SourceDiagnostic,
    ) {
        let location = diagnostic.span.and_then(|span| source.span(span));
        let target = location.and_then(|location| {
            snapshot
                .source(location.file)
                .filter(|source| source.revision() == location.revision)
                .map(|source| (source, location.range))
        });
        match target {
            Some((source, span)) => self.present(context, source, span, diagnostic),
            None => self.unlocated(source.name(), diagnostic),
        }
    }

    fn syntax(
        &mut self,
        context: &ContextCapture,
        source: &SourceFile,
        diagnostic: &SourceDiagnostic,
    ) {
        match diagnostic.span {
            Some(span) => self.present(context, source, span, diagnostic),
            None => self.unlocated(source.name(), diagnostic),
        }
    }

    fn unlocated(&mut self, source: &str, diagnostic: &SourceDiagnostic) {
        self.issue(format!(
            "{source}: {} (source location unavailable)",
            diagnostic.kind
        ));
    }

    fn present(
        &mut self,
        context: &ContextCapture,
        source: &SourceFile,
        span: Span,
        diagnostic: &SourceDiagnostic,
    ) {
        let uris = self.uris(context, source);
        let Some(range) = protocol_range(source, span) else {
            self.unlocated(source.name(), diagnostic);
            return;
        };
        if uris.is_empty() {
            self.unlocated(source.name(), diagnostic);
            return;
        }
        let value = Diagnostic {
            range,
            severity: Some(match diagnostic.severity {
                Severity::Error => DiagnosticSeverity::ERROR,
                Severity::Warning => DiagnosticSeverity::WARNING,
            }),
            code: Some(NumberOrString::String(diagnostic.kind.code().into())),
            source: Some("kagari".into()),
            message: diagnostic.kind.to_string(),
            ..Diagnostic::default()
        };
        for uri in uris {
            self.push(uri, value.clone());
        }
    }

    fn acquisition(&mut self, issue: &CaptureIssue, context: Option<&ContextCapture>) {
        let located = issue
            .uri
            .as_ref()
            .zip(issue.range.as_ref())
            .and_then(|(uri, span)| {
                let name = source_name(uri).ok()?;
                let source = self
                    .documents
                    .iter()
                    .find(|document| document.source_name == name)
                    .and_then(|document| document.source.as_deref())
                    .or_else(|| {
                        self.capture
                            .contexts
                            .iter()
                            .filter(|candidate| {
                                context.is_none_or(|context| context.key == candidate.key)
                            })
                            .find_map(|context| {
                                context
                                    .origins
                                    .iter()
                                    .find(|(_, origin)| origin.uri.as_ref() == Some(uri))
                                    .and_then(|(file, _)| context.sources.file(*file))
                                    .map(AsRef::as_ref)
                            })
                    })?;
                Some((
                    uri.clone(),
                    protocol_range(source, Span::new(span.start, span.end))?,
                ))
            });
        if let Some((uri, range)) = located {
            self.push(
                uri,
                Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::ERROR),
                    code: Some(NumberOrString::String("KG_PACKAGE_INPUT".into())),
                    source: Some("kagari".into()),
                    message: issue.message.clone(),
                    ..Diagnostic::default()
                },
            );
        } else {
            self.issue(issue.to_string());
        }
    }

    fn finish(self) -> Result<DiagnosticsResult, AnalysisFailure> {
        self.check()?;
        let groups = self
            .groups
            .into_iter()
            .map(|(uri, diagnostics)| {
                DiagnosticGroup::new(uri, diagnostics)
                    .map_err(|error| AnalysisFailure::Unavailable(error.to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if groups
            .iter()
            .map(DiagnosticGroup::retained_bytes)
            .sum::<usize>()
            > self.limits.bytes
        {
            return Err(AnalysisFailure::Capacity(
                "diagnostic measured result exceeds its retained-result budget".into(),
            ));
        }
        Ok(DiagnosticsResult {
            groups,
            issues: self.issues.into_iter().collect(),
        })
    }
}

pub(crate) fn protocol_range(source: &SourceFile, span: Span) -> Option<Range> {
    if span.start > span.end {
        return None;
    }
    let start = source.position(span.start, PositionEncoding::Utf16)?;
    let end = source.position(span.end, PositionEncoding::Utf16)?;
    Some(Range {
        start: Position {
            line: start.line.try_into().ok()?,
            character: start.character.try_into().ok()?,
        },
        end: Position {
            line: end.line.try_into().ok()?,
            character: end.character.try_into().ok()?,
        },
    })
}
