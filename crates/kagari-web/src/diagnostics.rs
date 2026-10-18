//! Presentation records use retained source identities, never guessed source offsets.
#[cfg(feature = "source")]
use crate::protocol::{TEXT_BYTES, prefix};
#[cfg(feature = "source")]
use kagari_source::{
    diagnostic::Severity, identity::FileSpan, line_index::PositionEncoding,
    presentation::DiagnosticRecord, source::SourceFile, source_database::SourceSnapshot,
};
use serde::Serialize;

#[derive(Serialize)]
pub(crate) struct Position {
    offset: usize,
    line: usize,
    character: usize,
}

#[derive(Serialize)]
pub(crate) struct Location {
    origin: String,
    revision: String,
    byte_start: usize,
    byte_end: usize,
    start: Position,
    end: Position,
}

#[derive(Serialize)]
pub(crate) struct Label {
    location: Option<Location>,
    message: String,
}

#[derive(Serialize)]
pub(crate) struct Diagnostic {
    code: String,
    severity: &'static str,
    message: String,
    location: Option<Location>,
    labels: Vec<Label>,
    notes: Vec<String>,
    truncated: bool,
}

#[cfg(feature = "source")]
fn position(source: &SourceFile, offset: usize) -> Option<Position> {
    let prefix = source.text().get(..offset)?;
    // An offset at LF inside CRLF is an absolute UTF-16 offset, but its editor
    // column is the preceding end of line (not a column inside the terminator).
    let column_offset =
        if prefix.ends_with('\r') && source.text().as_bytes().get(offset) == Some(&b'\n') {
            offset - 1
        } else {
            offset
        };
    let position = source.position(column_offset, PositionEncoding::Utf16)?;
    Some(Position {
        offset: prefix.encode_utf16().count(),
        line: position.line,
        character: position.character,
    })
}

#[cfg(feature = "source")]
fn location(span: FileSpan, snapshot: &SourceSnapshot) -> Option<Location> {
    let source = snapshot.file(span.file)?;
    if source.revision() != span.revision || span.range.start > span.range.end {
        return None;
    }
    Some(Location {
        origin: prefix(source.name(), TEXT_BYTES).to_owned(),
        revision: span.revision.0.to_string(),
        byte_start: span.range.start,
        byte_end: span.range.end,
        start: position(source, span.range.start)?,
        end: position(source, span.range.end)?,
    })
}

#[cfg(feature = "source")]
pub(crate) fn convert(record: &DiagnosticRecord, snapshot: &SourceSnapshot) -> Diagnostic {
    let mut truncated = record.code.len() > 128
        || record.message.len() > TEXT_BYTES
        || record.labels.len() > 8
        || record.notes.len() > 8;
    let labels = record
        .labels
        .iter()
        .take(8)
        .map(|label| {
            truncated |= label.message.len() > TEXT_BYTES;
            Label {
                location: location(label.span, snapshot),
                message: prefix(&label.message, TEXT_BYTES).to_owned(),
            }
        })
        .collect();
    let notes = record
        .notes
        .iter()
        .take(8)
        .map(|note| {
            truncated |= note.len() > TEXT_BYTES;
            prefix(note, TEXT_BYTES).to_owned()
        })
        .collect();
    Diagnostic {
        code: prefix(&record.code, 128).to_owned(),
        severity: match record.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        },
        message: prefix(&record.message, TEXT_BYTES).to_owned(),
        location: record.span.and_then(|span| location(span, snapshot)),
        labels,
        notes,
        truncated,
    }
}

#[cfg(all(test, feature = "source"))]
mod tests {
    use super::*;

    #[test]
    fn utf16_positions_preserve_unicode_and_crlf_offsets() {
        let source = SourceFile::new("memory://main.kgr", "a😀é\r\nb");
        for (byte, offset, line, character) in [
            (0, 0, 0, 0),
            (1, 1, 0, 1),
            (5, 3, 0, 3),
            (7, 4, 0, 4),
            (8, 5, 0, 4),
            (9, 6, 1, 0),
            (10, 7, 1, 1),
        ] {
            let actual = position(&source, byte).unwrap();
            assert_eq!(
                (actual.offset, actual.line, actual.character),
                (offset, line, character)
            );
        }
        assert!(position(&source, 2).is_none());
        assert!(position(&source, 11).is_none());
    }
}
