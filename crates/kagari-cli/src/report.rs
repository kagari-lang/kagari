//! Terminal and JSON rendering use only presentation records and retained text.

use crate::CliError;
use kagari_source::{
    diagnostic::Severity, identity::FileSpan, line_index::PositionEncoding,
    presentation::DiagnosticRecord, source::SourceFile, source_database::SourceSnapshot,
};
use serde::Serialize;
use serde_json::to_writer;
use std::{
    fmt::Write as _,
    io::{self, Write},
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum MessageFormat {
    #[default]
    Human,
    Json,
}

#[derive(Debug, Serialize)]
pub(crate) struct Location {
    path: String,
    revision: u64,
    byte_start: usize,
    byte_end: usize,
    start: Position,
    end: Position,
    #[serde(skip)]
    excerpt: String,
}

#[derive(Debug, Serialize)]
struct Position {
    line: usize,
    column: usize,
}

#[derive(Debug, Serialize)]
struct Label {
    location: Option<Location>,
    message: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct Record {
    #[serde(rename = "type")]
    kind: &'static str,
    context: String,
    code: String,
    severity: &'static str,
    message: String,
    location: Option<Location>,
    labels: Vec<Label>,
    notes: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Summary {
    #[serde(rename = "type")]
    kind: &'static str,
    command: &'static str,
    pub succeeded: usize,
    pub failed: usize,
    pub incomplete: usize,
    pub exit_code: u8,
}

pub(crate) struct Report {
    records: Vec<Record>,
    pub summary: Summary,
}

impl Report {
    pub fn new(command: &'static str) -> Self {
        Self {
            records: Vec::new(),
            summary: Summary {
                kind: "summary",
                command,
                succeeded: 0,
                failed: 0,
                incomplete: 0,
                exit_code: 0,
            },
        }
    }

    pub fn diagnostic(
        &mut self,
        context: &str,
        diagnostic: DiagnosticRecord,
        sources: &SourceSnapshot,
    ) {
        self.with_sources(context, diagnostic, |span| {
            sources.file(span.file).map(|source| source.as_ref())
        });
    }

    pub fn source_diagnostic(
        &mut self,
        context: &str,
        diagnostic: DiagnosticRecord,
        source: &SourceFile,
    ) {
        self.with_sources(context, diagnostic, |_| Some(source));
    }

    fn with_sources<'a>(
        &mut self,
        context: &str,
        diagnostic: DiagnosticRecord,
        source: impl Fn(FileSpan) -> Option<&'a SourceFile>,
    ) {
        let mut notes = diagnostic.notes;
        let mut locate = |span| {
            let result = source(span).and_then(|source| location(span, source));
            if result.is_none() {
                notes.push("source location unavailable: retained source identity, revision or range does not match".into());
            }
            result
        };
        let primary = diagnostic.span.and_then(&mut locate);
        let labels = diagnostic
            .labels
            .into_iter()
            .map(|label| Label {
                location: locate(label.span),
                message: label.message,
            })
            .collect();
        self.records.push(Record {
            kind: "diagnostic",
            context: context.into(),
            code: diagnostic.code,
            severity: match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            },
            message: diagnostic.message,
            location: primary,
            labels,
            notes,
        });
    }

    pub fn operational(&mut self, context: &str, code: &str, message: impl Into<String>) {
        self.records.push(Record {
            kind: "diagnostic",
            context: context.into(),
            code: code.into(),
            severity: "error",
            message: message.into(),
            location: None,
            labels: Vec::new(),
            notes: Vec::new(),
        });
    }

    pub fn finish(mut self, format: MessageFormat) -> Result<(), CliError> {
        if self.summary.exit_code == 0 && (self.summary.failed > 0 || self.summary.incomplete > 0) {
            self.summary.exit_code = 1;
        }
        self.records.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        let write = || -> io::Result<()> {
            match format {
                MessageFormat::Json => {
                    let stdout = io::stdout();
                    let mut output = stdout.lock();
                    for record in &self.records {
                        to_writer(&mut output, record)?;
                        writeln!(output)?;
                    }
                    to_writer(&mut output, &self.summary)?;
                    writeln!(output)
                }
                MessageFormat::Human => {
                    let stderr = io::stderr();
                    let mut output = stderr.lock();
                    for record in &self.records {
                        writeln!(output, "{}", human(record))?;
                    }
                    writeln!(
                        output,
                        "{}: {} succeeded, {} failed, {} incomplete",
                        self.summary.command,
                        self.summary.succeeded,
                        self.summary.failed,
                        self.summary.incomplete
                    )
                }
            }
        };
        write().map_err(|error| CliError::message(1, error.to_string()))?;
        if self.summary.exit_code == 0 {
            Ok(())
        } else {
            Err(CliError::message(
                self.summary.exit_code,
                self.records
                    .first()
                    .map(|record| format!("diagnostics emitted: {}", record.message))
                    .unwrap_or_else(|| "diagnostics emitted".into()),
            ))
        }
    }
}

fn sort_key(record: &Record) -> (&str, usize, &str, &str, &str, &str) {
    let (path, offset) = record
        .location
        .as_ref()
        .map(|location| (location.path.as_str(), location.byte_start))
        .unwrap_or(("", 0));
    (
        path,
        offset,
        record.severity,
        &record.code,
        &record.context,
        &record.message,
    )
}

fn location(span: FileSpan, source: &SourceFile) -> Option<Location> {
    if span.file != source.origin_id()
        || span.revision != source.revision()
        || source.span(span.range)? != span
    {
        return None;
    }
    let start = source.position(span.range.start, PositionEncoding::Utf8)?;
    let end = source.position(span.range.end, PositionEncoding::Utf8)?;
    let line = source.text().lines().nth(start.line).unwrap_or("");
    Some(Location {
        path: source.name().replace('\\', "/"),
        revision: span.revision.0,
        byte_start: span.range.start,
        byte_end: span.range.end,
        start: Position {
            line: start.line,
            column: start.character,
        },
        end: Position {
            line: end.line,
            column: end.character,
        },
        excerpt: line.into(),
    })
}

fn human(record: &Record) -> String {
    let mut text = format!("{}[{}]: {}", record.severity, record.code, record.message);
    if !record.context.is_empty() {
        let _ = write!(text, " ({})", record.context);
    }
    if let Some(location) = &record.location {
        render_location(&mut text, location, "primary");
    }
    for label in &record.labels {
        if let Some(location) = &label.location {
            if label.message != "primary" {
                render_location(&mut text, location, &label.message);
            }
        } else {
            let _ = write!(text, "\n  = {} (location unavailable)", label.message);
        }
    }
    for note in &record.notes {
        let _ = write!(text, "\n  = {note}");
    }
    text
}

fn render_location(text: &mut String, location: &Location, label: &str) {
    let mut column = 0;
    let mut expanded = String::new();
    let mut caret = 0;
    for (offset, ch) in location.excerpt.char_indices() {
        if offset < location.start.column {
            caret = column + if ch == '\t' { 4 - column % 4 } else { 1 };
        }
        if ch == '\t' {
            let spaces = 4 - column % 4;
            expanded.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            expanded.push(ch);
            column += 1;
        }
    }
    let _ = write!(
        text,
        "\n  --> {}:{}:{}\n   | {expanded}\n   | {}^ {label}",
        location.path,
        location.start.line + 1,
        location.start.column + 1,
        " ".repeat(caret)
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_common::span::Span;
    use kagari_source::diagnostic::{Diagnostic, DiagnosticKind};

    #[test]
    fn rendering_uses_retained_revision_without_hir_or_process_ids() {
        let source = SourceFile::new("unicode.kgr", "// π\r\n\tbroken\n");
        let diagnostic =
            Diagnostic::error(DiagnosticKind::UnexpectedToken).with_span(Span::new(8, 14));
        let mut record = DiagnosticRecord::from_diagnostic(diagnostic, &source);
        let mut report = Report::new("check");
        report.source_diagnostic("app", record.clone(), &source);
        let json = serde_json::to_value(&report.records[0]).unwrap();
        assert_eq!(json["location"]["start"]["line"], 1);
        assert_eq!(json["location"]["start"]["column"], 1);
        assert!(json["location"].get("file").is_none());
        assert!(human(&report.records[0]).contains("    broken"));
        record.span.as_mut().unwrap().revision.0 += 1;
        record.labels.clear();
        report.source_diagnostic("app", record, &source);
        assert!(report.records[1].location.is_none());
        assert!(!report.records[1].notes.is_empty());
        assert!(!human(&report.records[1]).contains("broken"));
    }
}
