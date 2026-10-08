use serde::{Deserialize, Serialize};

/// A half-open byte range `[start, end)` in source text.
///
/// `start` is included and `end` is excluded. Offsets count UTF-8 bytes, not
/// characters or line/column positions. For example, in `a>>2`, the two `>`
/// tokens occupy `[1, 2)` and `[2, 3)`; together they occupy `[1, 3)`.
/// Adjacent ranges therefore satisfy `left.end == right.start`.
///
/// An empty range has `start == end`; EOF uses `[text.len(), text.len())`.
/// A span carries no file or revision identity: its owner supplies that context.
/// Construction does not validate ordering, text bounds or UTF-8 boundaries;
/// callers must establish those before slicing `&text[start..end]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    /// Inclusive starting byte offset.
    pub start: usize,
    /// Exclusive ending byte offset.
    pub end: usize,
}

impl Span {
    /// Records byte offsets without validating them against source text.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}
