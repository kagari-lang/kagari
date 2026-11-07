#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionEncoding {
    Utf8,
    Utf16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub character: usize,
}

/// Strict byte/position conversion over retained source text.
/// LF, CRLF and lone CR each delimit a line without changing the source bytes.
/// The interior of CRLF and split Unicode scalars are not valid positions.
#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
    ends: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        let mut ends = Vec::new();
        let mut bytes = text.bytes().enumerate().peekable();
        while let Some((offset, byte)) = bytes.next() {
            if byte != b'\n' && byte != b'\r' {
                continue;
            }
            ends.push(offset);
            let width = if byte == b'\r' && bytes.peek().is_some_and(|(_, next)| *next == b'\n') {
                bytes.next();
                2
            } else {
                1
            };
            starts.push(offset + width);
        }
        ends.push(text.len());
        Self { starts, ends }
    }

    /// Returns a line without its LF, CRLF or CR delimiter, including the final empty line.
    pub fn line<'a>(&self, text: &'a str, line: usize) -> Option<&'a str> {
        text.get(*self.starts.get(line)?..*self.ends.get(line)?)
    }

    pub(crate) fn retained_bytes(&self) -> usize {
        (self.starts.capacity() + self.ends.capacity()) * size_of::<usize>()
    }

    pub fn position(
        &self,
        text: &str,
        offset: usize,
        encoding: PositionEncoding,
    ) -> Option<Position> {
        if !text.is_char_boundary(offset) {
            return None;
        }
        let line = self
            .starts
            .partition_point(|start| *start <= offset)
            .checked_sub(1)?;
        if offset > self.ends[line] {
            return None;
        }
        let prefix = text.get(self.starts[line]..offset)?;
        let character = match encoding {
            PositionEncoding::Utf8 => prefix.len(),
            PositionEncoding::Utf16 => prefix.encode_utf16().count(),
        };
        Some(Position { line, character })
    }

    pub fn offset(
        &self,
        text: &str,
        position: Position,
        encoding: PositionEncoding,
    ) -> Option<usize> {
        let start = *self.starts.get(position.line)?;
        let line = self.line(text, position.line)?;
        match encoding {
            PositionEncoding::Utf8 => (position.character <= line.len()
                && line.is_char_boundary(position.character))
            .then_some(start + position.character),
            PositionEncoding::Utf16 => {
                let mut units = 0;
                for (offset, ch) in line.char_indices() {
                    if units == position.character {
                        return Some(start + offset);
                    }
                    units += ch.len_utf16();
                    if units > position.character {
                        return None;
                    }
                }
                (units == position.character).then_some(start + line.len())
            }
        }
    }
}
