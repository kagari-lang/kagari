//! Rowan tree handles specialized to Kagari's syntax vocabulary.
//!
//! Green nodes own immutable kinds/token text. Traversable node handles add
//! parent/child navigation and byte ranges relative to the tree root; typed AST
//! wrappers retain these handles. File identity is supplied by the caller.
//! Raw trees must contain only valid [`SyntaxKind`] discriminants: the language
//! adapter's raw-kind conversion relies on that invariant, not on input recovery.

use rowan::{
    GreenNode, Language, NodeOrToken, SyntaxKind as RawSyntaxKind, SyntaxNode as RawSyntaxNode,
    SyntaxNodeChildren as RawSyntaxNodeChildren, SyntaxToken as RawSyntaxToken,
};
use std::mem;

use crate::kind::SyntaxKind;

/// Type-level marker connecting Rowan nodes and tokens to [`SyntaxKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KagariLanguage {}

impl Language for KagariLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: RawSyntaxKind) -> Self::Kind {
        // SAFETY: every node/token kind in the tree is created from SyntaxKind.
        unsafe { mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> RawSyntaxKind {
        RawSyntaxKind(kind as u16)
    }
}

/// A retained, navigable CST node; child access does not perform semantic analysis.
pub type SyntaxNode = RawSyntaxNode<KagariLanguage>;
/// A CST leaf retaining its kind, original text and byte range.
pub type SyntaxToken = RawSyntaxToken<KagariLanguage>;
/// One child element: either a node or a token, including trivia tokens.
pub type SyntaxElement = NodeOrToken<SyntaxNode, SyntaxToken>;
/// Direct child nodes in source order, excluding tokens.
pub type SyntaxNodeChildren = RawSyntaxNodeChildren<KagariLanguage>;

/// Creates a root handle over an owned green tree of valid Kagari syntax kinds.
///
/// The resulting handle retains the tree; it does not borrow the input source.
/// The root kind is unchanged, so use [`crate::ast::traits::AstNode::cast`] to
/// select a typed view. This function does not validate arbitrary raw kinds.
pub fn syntax_node_from_green(green: GreenNode) -> SyntaxNode {
    SyntaxNode::new_root(green)
}
