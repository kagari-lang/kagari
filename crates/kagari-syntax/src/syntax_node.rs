use rowan::{GreenNode, Language};

use crate::kind::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KagariLanguage {}

impl Language for KagariLanguage {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        // SAFETY: every node/token kind in the tree is created from SyntaxKind.
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        rowan::SyntaxKind(kind as u16)
    }
}

pub type SyntaxNode = rowan::SyntaxNode<KagariLanguage>;
pub type SyntaxToken = rowan::SyntaxToken<KagariLanguage>;
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;
pub type SyntaxNodeChildren = rowan::SyntaxNodeChildren<KagariLanguage>;

pub fn syntax_node_from_green(green: GreenNode) -> SyntaxNode {
    SyntaxNode::new_root(green)
}
