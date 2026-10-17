//! Presentation-neutral diagnostics, independent of semantic analysis and runtime.

use crate::{
    diagnostic::{Diagnostic, Severity},
    identity::FileSpan,
    source::SourceFile,
};

/// A final-severity diagnostic with revision-bound locations, labels and notes.
/// Language producers retain typed errors internally and adapt them at their
/// reporting boundary; optional tools may construct records with their own codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub severity: Severity,
    pub code: String,
    pub span: Option<FileSpan>,
    pub message: String,
    pub notes: Vec<String>,
    pub labels: Vec<DiagnosticLabel>,
}

impl DiagnosticRecord {
    /// Binds a language diagnostic to the captured source that produced its ranges.
    /// Invalid ranges become spanless; consumers must still validate identity and
    /// revision before looking up excerpts in another retained source collection.
    pub fn from_diagnostic(diagnostic: Diagnostic, source: &SourceFile) -> Self {
        let span = diagnostic.span.and_then(|span| source.span(span));
        Self {
            severity: diagnostic.severity,
            code: diagnostic.kind.code().to_owned(),
            span,
            message: diagnostic.kind.to_string(),
            notes: Vec::new(),
            labels: span
                .into_iter()
                .map(|span| DiagnosticLabel {
                    span,
                    message: "primary".to_owned(),
                })
                .collect(),
        }
    }
}

/// A labeled location in the same or another retained source revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticLabel {
    pub span: FileSpan,
    pub message: String,
}
