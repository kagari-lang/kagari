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

#[derive(Debug, Clone)]
pub struct LineIndex {
    starts: Vec<usize>,
    ends: Vec<usize>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut starts = vec![0];
        let mut ends = Vec::new();
        for (offset, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                ends.push(if offset > 0 && text.as_bytes()[offset - 1] == b'\r' {
                    offset - 1
                } else {
                    offset
                });
                starts.push(offset + 1);
            }
        }
        ends.push(text.len());
        Self { starts, ends }
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
        let line = text.get(start..*self.ends.get(position.line)?)?;
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
