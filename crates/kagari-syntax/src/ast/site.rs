//! Parse-local source coverage, missing grammar slots and separated lists.

use crate::ast::tokens::{Comma, Token};

/// Coverage in the owning parse's lexical table, with sparse recovery indirection.
/// Empty coverage anchors missing/empty constructs at the next lexical boundary.
/// This is neither a byte span nor an ID transferable between parses. Resolve
/// source ranges through [`Ast`](crate::ast::borrow::Ast) with the original owner.
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
///
/// `Option<T>` describes an optional construct; `Required<T>` describes a role
/// that the selected grammar needs, even when its source is absent. For
/// `Option<Required<T>>`, `None` and `Some(Missing { .. })` therefore differ.
/// For example, no `else` clause differs from a written `else` with no body.
#[derive(Debug)]
pub enum Required<T> {
    /// A recovered value exists; its own children may still be missing or invalid.
    Present(T),
    /// A zero-width insertion anchor in physical-source UTF-8 bytes, possibly at EOF.
    /// It is not a token index, and no lexical token is synthesized at this position.
    Missing { at: u32 },
}

impl<T> Required<T> {
    pub(crate) fn map<U>(self, map: impl FnOnce(T) -> U) -> Required<U> {
        match self {
            Self::Present(value) => Required::Present(map(value)),
            Self::Missing { at } => Required::Missing { at },
        }
    }

    /// Borrow a present value, discarding the missing-role anchor from this projection.
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Present(value) => Some(value),
            Self::Missing { .. } => None,
        }
    }
}

/// Ordered entries retain missing elements, missing separators and trailing punctuation.
/// For `f(a,,b,)`, entries are `a,`, missing-at-second-comma, and `b,`.
///
/// ```text
/// source:             f(a,,b,)
/// entries[0]:         Present(a), separator = Some(Present(',' token))
/// entries[1]:         Missing,    separator = Some(Present(',' token))
/// entries[2]:         Present(b), separator = Some(Present(',' token))
/// call.args():        a, b        (missing entries are filtered out)
/// ```
///
/// Entries refer to nodes and real separator tokens separately. Typed list
/// accessors such as `args()` omit missing elements; use `Ast::data()` and this
/// entry slice when recovery positions or exact punctuation matter. The second
/// returned argument above has original entry index 2, not 1.
///
/// For `f(a b)`, the first entry instead has `Some(Missing { at })` anchored
/// at `b`; the second has no separator. For `f(a @@@,b)`, the comma is present
/// and `@@@` belongs to the first entry's [`RecoveryPart::Separator`], exposed
/// through the owning node's `unexpected()` rather than as another element.
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

/// One authored list slot, retaining its element and separator independently.
#[derive(Debug)]
pub struct ListEntry<T, S = Comma> {
    /// Required element, including an explicit missing slot in malformed input.
    pub element: Required<T>,
    /// `None`: no separator role retained, normally the final entry.
    /// `Some(Present(_))`: written punctuation, including a trailing separator.
    /// `Some(Missing { .. })`: grammar required a separator but none was written.
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

/// Which portion of a field/list entry owns skipped lexical input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPart {
    /// Input assigned to the element position, before its retained value.
    Element,
    /// Input assigned after an element and before its separator.
    Separator,
}

/// Insertion among the owner's named Rust fields and zero-based list entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryPosition {
    /// Owner's grammar field name; `unparsed_suffix` identifies input left after
    /// a parser stop, and `input` is used when no named field is available.
    pub field: &'static str,
    /// Index in the complete owning list, including missing slots, or zero for
    /// a singular field. It may equal the list length for input after its entries.
    pub entry: usize,
    /// Whether skipped input precedes the entry's element or separator.
    pub part: RecoveryPart,
}
