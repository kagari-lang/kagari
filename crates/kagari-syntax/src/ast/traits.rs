use crate::{kind::SyntaxKind, syntax_node::SyntaxNode};

pub trait AstNode: Sized {
    fn can_cast(kind: SyntaxKind) -> bool;

    fn cast(syntax: SyntaxNode) -> Option<Self>;

    fn syntax(&self) -> &SyntaxNode;
}
