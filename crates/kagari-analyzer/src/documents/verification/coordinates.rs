//! Scalar-walking oracle; no expected value comes from the source line index.
use kagari_common::span::Span;
use kagari_source::{
    line_index::{Position, PositionEncoding},
    source::SourceFile,
};

struct Boundary {
    byte: usize,
    line: usize,
    utf8: usize,
    utf16: usize,
}

impl Boundary {
    fn position(&self, encoding: PositionEncoding) -> Position {
        Position {
            line: self.line,
            character: match encoding {
                PositionEncoding::Utf8 => self.utf8,
                PositionEncoding::Utf16 => self.utf16,
            },
        }
    }
}

pub(super) fn check(file: &SourceFile, text: &str, probe: u8) {
    assert_eq!(file.text(), text);
    let mut boundaries = vec![Boundary {
        byte: 0,
        line: 0,
        utf8: 0,
        utf16: 0,
    }];
    let mut lines = vec![String::new()];
    let mut chars = text.char_indices().peekable();
    let mut utf8 = 0;
    let mut utf16 = 0;
    while let Some((offset, scalar)) = chars.next() {
        let mut byte = offset + scalar.len_utf8();
        match scalar {
            '\r' | '\n' => {
                if scalar == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    byte = chars.next().unwrap().0 + 1;
                }
                lines.push(String::new());
                utf8 = 0;
                utf16 = 0;
            }
            _ => {
                lines.last_mut().unwrap().push(scalar);
                utf8 += scalar.len_utf8();
                utf16 += scalar.len_utf16();
            }
        }
        boundaries.push(Boundary {
            byte,
            line: lines.len() - 1,
            utf8,
            utf16,
        });
    }
    for encoding in [PositionEncoding::Utf8, PositionEncoding::Utf16] {
        for byte in 0..=text.len() + 1 {
            let expected = boundaries
                .iter()
                .find(|boundary| boundary.byte == byte)
                .map(|boundary| boundary.position(encoding));
            assert_eq!(
                file.position(byte, encoding),
                expected,
                "byte {byte}, {encoding:?}"
            );
        }
        for (line, content) in lines.iter().enumerate() {
            assert_eq!(file.line(line), Some(content.as_str()));
            let length = match encoding {
                PositionEncoding::Utf8 => content.len(),
                PositionEncoding::Utf16 => content.encode_utf16().count(),
            };
            for character in 0..=length + 1 {
                let position = Position { line, character };
                let expected = boundaries
                    .iter()
                    .find(|boundary| boundary.position(encoding) == position)
                    .map(|boundary| boundary.byte);
                assert_eq!(
                    file.offset(position, encoding),
                    expected,
                    "{position:?}, {encoding:?}"
                );
            }
        }
        assert_eq!(
            file.offset(
                Position {
                    line: lines.len(),
                    character: 0
                },
                encoding
            ),
            None
        );
    }
    assert_eq!(file.line(lines.len()), None);
    let selected = usize::from(probe) % boundaries.len();
    for (start, end) in [(0, selected), (selected, boundaries.len() - 1)] {
        let range = Span::new(boundaries[start].byte, boundaries[end].byte);
        let span = file.span(range).unwrap();
        assert_eq!(span.file, file.id());
        assert_eq!(span.revision, file.revision());
        assert_eq!(span.range, range);
        assert_eq!(
            &file.text()[span.range.start..span.range.end],
            &text[range.start..range.end]
        );
        for boundary in [&boundaries[start], &boundaries[end]] {
            assert_eq!(
                file.offset(
                    boundary.position(PositionEncoding::Utf16),
                    PositionEncoding::Utf16
                ),
                Some(boundary.byte)
            );
        }
    }
}
