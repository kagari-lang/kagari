//! Parse-local source coverage, missing grammar slots and separated lists.

use crate::ast::tokens::{Comma, Token};

/// Coverage in the owning parse's lexical table, with sparse recovery indirection.
/// Empty coverage anchors missing/empty constructs at the next lexical boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxSite {
    pub(crate) first: u32,
    pub(crate) end: u32,
}

/// Recovery-only coverage and skipped contents, allocated only for damaged nodes.
#[derive(Debug)]
pub(crate) struct Recovery {
    pub site: SyntaxSite,
    pub unexpected: Vec<Unexpected>,
}

impl SyntaxSite {
    /// Ordered coordinates are inline. Reversed coordinates encode a one-based
    /// recovery-table index, leaving the ordinary representation at eight bytes.
    pub(crate) fn resolve(self, recoveries: &[Recovery]) -> Self {
        if self.first > self.end {
            recoveries[self.first as usize - 1].site
        } else {
            self
        }
    }

    pub(crate) fn records(self, recoveries: &[Recovery]) -> &[Unexpected] {
        if self.first > self.end {
            &recoveries[self.first as usize - 1].unexpected
        } else {
            &[]
        }
    }

    pub(crate) fn token(index: u32) -> Self {
        Self {
            first: index,
            end: index + 1,
        }
    }
}

/// A grammatical requirement; missing input never borrows the next field's value.
#[derive(Debug)]
pub enum Required<T> {
    Present(T),
    Missing { at: u32 },
}

impl<T> Required<T> {
    pub(crate) fn map<U>(self, map: impl FnOnce(T) -> U) -> Required<U> {
        match self {
            Self::Present(value) => Required::Present(map(value)),
            Self::Missing { at } => Required::Missing { at },
        }
    }

    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Missing { .. } => None,
        }
    }
}

/// Ordered entries retain missing elements, missing separators and trailing punctuation.
/// For `f(a,,b,)`, entries are `a,`, missing-at-second-comma, and `b,`.
#[derive(Debug)]
pub struct SeparatedList<T, S = Comma> {
    pub entries: Box<[ListEntry<T, S>]>,
}

impl<T, S> Default for SeparatedList<T, S> {
    fn default() -> Self {
        Self {
            entries: Box::new([]),
        }
    }
}

impl<T, S> From<Vec<ListEntry<T, S>>> for SeparatedList<T, S> {
    fn from(entries: Vec<ListEntry<T, S>>) -> Self {
        Self {
            entries: entries.into_boxed_slice(),
        }
    }
}

#[derive(Debug)]
pub struct ListEntry<T, S = Comma> {
    pub element: Required<T>,
    pub separator: Option<Required<Token<S>>>,
}

/// Unexpected lexical contents at a parser-assigned grammatical insertion position.
#[derive(Debug)]
pub(crate) struct Unexpected {
    pub field: u16,
    pub entry: u32,
    pub part: RecoveryPart,
    pub first: u32,
    pub end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPart {
    Element,
    Separator,
}

/// Insertion among the owner's named Rust fields and zero-based list entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPosition {
    pub field: &'static str,
    pub entry: usize,
    pub part: RecoveryPart,
}
