//! One retained lexical table, with shared source spelling and deterministic trivia boundaries.

use std::collections::HashMap;

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    span::Span,
};

use crate::{TokenBuffer, ast::site::SyntaxSite, token::TokenKind};

#[derive(Debug)]
pub(crate) struct Lexical {
    starts: Offsets,
    kinds: Box<[TokenKind]>,
    boundaries: HashMap<u32, u32>,
    length: u32,
}

/// Offsets keep byte semantics at either storage width. The source length proves
/// every start fits before narrowing; large files retain full 32-bit coordinates.
#[derive(Debug)]
enum Offsets {
    Narrow(Box<[u16]>),
    Wide(Box<[u32]>),
}

impl Offsets {
    fn get(&self, index: usize) -> Option<u32> {
        match self {
            Self::Narrow(starts) => starts.get(index).copied().map(u32::from),
            Self::Wide(starts) => starts.get(index).copied(),
        }
    }
}

impl Lexical {
    pub(crate) fn new(
        text: &str,
        tokens: TokenBuffer,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let length = u32::try_from(text.len()).expect("32-bit syntax coordinates");
        u32::try_from(tokens.len()).expect("32-bit lexical coordinates");
        // Immutable parse-owned exceptions preserve expected constant-time full
        // extent reads without storing another offset on every lexical record.
        let mut boundaries = HashMap::new();
        let mut previous = None;
        for (index, token) in tokens.iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            if token.kind.is_trivia() {
                continue;
            }
            let first = previous.map_or(0, |index| index + 1);
            let mut boundary = token.span.start;
            if previous.is_some() {
                for (offset, trivia) in tokens[first..index].iter().enumerate() {
                    if offset % 1024 == 0 {
                        cancel.check()?;
                    }
                    let spelling = &text[trivia.span.start..trivia.span.end];
                    if let Some(newline) = spelling.find(['\r', '\n']) {
                        boundary = if trivia.kind == TokenKind::BlockComment {
                            trivia.span.end
                        } else {
                            let mut end = newline + 1;
                            if spelling.as_bytes()[newline] == b'\r'
                                && spelling.as_bytes().get(end) == Some(&b'\n')
                            {
                                end += 1;
                            }
                            trivia.span.start + end
                        };
                        break;
                    }
                }
            } else {
                boundary = 0;
            }
            for position in first..=index {
                if position % 1024 == 0 {
                    cancel.check()?;
                }
                let default = if tokens[position].kind.is_trivia() {
                    tokens
                        .get(position + 1)
                        .map_or(text.len(), |token| token.span.start)
                } else {
                    tokens[position].span.start
                };
                if default == boundary {
                    continue;
                }
                let offset = u32::try_from(boundary).expect("32-bit syntax coordinates");
                boundaries.insert(position as u32, offset);
            }
            previous = Some(index);
        }
        let mut starts = Vec::with_capacity(tokens.len());
        let mut kinds = Vec::with_capacity(tokens.len());
        for (index, token) in tokens.into_iter().enumerate() {
            if index % 1024 == 0 {
                cancel.check()?;
            }
            starts.push(
                token
                    .span
                    .start
                    .try_into()
                    .expect("32-bit syntax coordinates"),
            );
            kinds.push(token.kind);
        }
        let starts = if u16::try_from(text.len()).is_ok() {
            Offsets::Narrow(
                starts
                    .into_iter()
                    .map(|offset: u32| {
                        u16::try_from(offset).expect("source length bounds lexical starts")
                    })
                    .collect(),
            )
        } else {
            Offsets::Wide(starts.into_boxed_slice())
        };
        Ok(Self {
            starts,
            kinds: kinds.into_boxed_slice(),
            boundaries,
            length,
        })
    }

    fn boundary(&self, index: u32) -> u32 {
        if let Some(offset) = self.boundaries.get(&index) {
            *offset
        } else if self.kind(index as usize).is_trivia() {
            self.offset(index as usize + 1)
        } else {
            self.offset(index as usize)
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.kinds.len()
    }

    pub(crate) fn kind(&self, index: usize) -> TokenKind {
        self.kinds.get(index).cloned().unwrap_or(TokenKind::Eof)
    }

    pub(crate) fn offset(&self, index: usize) -> u32 {
        self.starts.get(index).unwrap_or(self.length)
    }

    pub(crate) fn span(&self, index: usize) -> Span {
        Span::new(self.offset(index) as usize, self.offset(index + 1) as usize)
    }

    pub(crate) fn body_span(&self, site: SyntaxSite) -> Span {
        Span::new(
            self.offset(site.first as usize) as usize,
            self.offset(site.end as usize) as usize,
        )
    }

    pub(crate) fn full_span(&self, site: SyntaxSite) -> Span {
        if site.first == site.end {
            return self.body_span(site);
        }
        let start = self.boundary(site.first);
        let end = self.boundary(site.end);
        Span::new(start as usize, end as usize)
    }
}
