use rowan::{GreenNode, Language};
use rowan::{
    NodeOrToken, SyntaxKind as RawSyntaxKind, SyntaxNode as RawSyntaxNode,
    SyntaxNodeChildren as RawSyntaxNodeChildren, SyntaxToken as RawSyntaxToken,
};
use std::mem;

use crate::kind::SyntaxKind;

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

pub type SyntaxNode = RawSyntaxNode<KagariLanguage>;
pub type SyntaxToken = RawSyntaxToken<KagariLanguage>;
pub type SyntaxElement = NodeOrToken<SyntaxNode, SyntaxToken>;
pub type SyntaxNodeChildren = RawSyntaxNodeChildren<KagariLanguage>;

pub fn syntax_node_from_green(green: GreenNode) -> SyntaxNode {
    SyntaxNode::new_root(green)
}
