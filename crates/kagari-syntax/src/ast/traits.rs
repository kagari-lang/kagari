//! The common conversion and source access contract for typed AST views.

use crate::{kind::SyntaxKind, syntax_node::SyntaxNode};

/// A typed handle to a syntax node; casting establishes its kind, not validity.
///
/// See [`crate::ast`] for storage, traversal and recovery conventions and
/// [`crate`] for a complete parse/inspect example.
pub trait AstNode: Sized {
    /// Whether this view accepts `kind`; sum types can accept several kinds.
    fn can_cast(kind: SyntaxKind) -> bool;

    /// Wraps a matching node without rebuilding it; returns `None` for other kinds.
    fn cast(syntax: SyntaxNode) -> Option<Self>;

    /// Borrows the underlying node, including punctuation, trivia and error children.
    fn syntax(&self) -> &SyntaxNode;

    /// Consecutive outer line documentation preceding this declaration.
    ///
    /// `source` must be the full text corresponding to this tree's byte offsets.
    /// Scans preceding lines backwards until a non-`///` line, strips the marker
    /// and at most one space, and joins lines with newlines. Returns an empty
    /// string if no documentation is found or the start offset cannot be sliced.
    fn documentation(&self, source: &str) -> String {
        let start = usize::from(self.syntax().text_range().start());
        let Some(prefix) = source.get(..start) else {
            return String::new();
        };
        let mut lines = prefix.lines().rev();
        let mut docs = Vec::new();
        if let Some(last) = lines.next()
            && !last.trim().is_empty()
        {
            if let Some(doc) = last.trim_start().strip_prefix("///") {
                docs.push(doc.strip_prefix(' ').unwrap_or(doc));
            } else {
                return String::new();
            }
        }
        for line in lines {
            let Some(doc) = line.trim_start().strip_prefix("///") else {
                break;
            };
            docs.push(doc.strip_prefix(' ').unwrap_or(doc));
        }
        docs.reverse();
        docs.join("\n")
    }
}
