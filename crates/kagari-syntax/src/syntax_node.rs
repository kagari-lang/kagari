//! Rowan tree handles specialized to Kagari's syntax vocabulary.
//!
//! Green nodes own immutable kinds/token text. Traversable node handles add
//! parent/child navigation and byte ranges relative to the tree root; typed AST
//! wrappers retain these handles. File identity is supplied by the caller.
//! Raw kinds are checked before conversion to [`SyntaxKind`]. Invalid external
//! kinds panic rather than constructing an invalid enum; this is distinct from
//! parsing malformed source into valid recovery nodes.

use rowan::{
    GreenNode, Language, NodeOrToken, SyntaxKind as RawSyntaxKind, SyntaxNode as RawSyntaxNode,
    SyntaxNodeChildren as RawSyntaxNodeChildren, SyntaxToken as RawSyntaxToken,
};

use crate::kind::SyntaxKind;

/// Type-level marker connecting Rowan nodes and tokens to [`SyntaxKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KagariLanguage {}

impl Language for KagariLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: RawSyntaxKind) -> Self::Kind {
        SyntaxKind::from_raw(raw.0).expect("invalid Kagari syntax kind")
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
/// select a typed view. The root kind is checked immediately; descendant kinds
/// are checked when accessed, without traversing the entire tree here.
///
/// # Panics
///
/// Panics if the root's raw kind is not a valid Kagari kind. Accessing an invalid
/// descendant kind also panics; this function does not validate the CST's grammar.
pub fn syntax_node_from_green(green: GreenNode) -> SyntaxNode {
    KagariLanguage::kind_from_raw(green.kind());
    SyntaxNode::new_root(green)
}

#[cfg(test)]
mod tests {
    use super::{KagariLanguage, syntax_node_from_green};
    use crate::kind::SyntaxKind;
    use rowan::{GreenNodeBuilder, Language, SyntaxKind as RawSyntaxKind};
    use std::panic::catch_unwind;

    #[test]
    fn raw_kinds_round_trip_and_invalid_values_are_rejected() {
        for raw in 0..=u16::MAX {
            if raw <= SyntaxKind::Error as u16 {
                let kind = KagariLanguage::kind_from_raw(RawSyntaxKind(raw));
                assert_eq!(KagariLanguage::kind_to_raw(kind), RawSyntaxKind(raw));
            } else {
                assert!(SyntaxKind::from_raw(raw).is_none());
            }
        }
        for raw in [SyntaxKind::Error as u16 + 1, u16::MAX] {
            assert!(catch_unwind(|| KagariLanguage::kind_from_raw(RawSyntaxKind(raw))).is_err());
        }
    }

    #[test]
    fn green_tree_access_rejects_invalid_node_and_token_kinds() {
        let mut builder = GreenNodeBuilder::new();
        builder.start_node(RawSyntaxKind(u16::MAX));
        builder.finish_node();
        let invalid_root = builder.finish();
        assert!(catch_unwind(|| syntax_node_from_green(invalid_root)).is_err());

        let mut builder = GreenNodeBuilder::new();
        builder.start_node(KagariLanguage::kind_to_raw(SyntaxKind::SourceFile));
        builder.start_node(RawSyntaxKind(u16::MAX));
        builder.finish_node();
        builder.token(RawSyntaxKind(u16::MAX), "invalid");
        builder.finish_node();
        let green = builder.finish();
        assert_eq!(
            syntax_node_from_green(green.clone()).kind(),
            SyntaxKind::SourceFile
        );
        assert!(
            catch_unwind(|| syntax_node_from_green(green.clone())
                .first_child()
                .unwrap()
                .kind())
            .is_err()
        );
        assert!(
            catch_unwind(|| syntax_node_from_green(green.clone())
                .last_token()
                .unwrap()
                .kind())
            .is_err()
        );
    }
}
