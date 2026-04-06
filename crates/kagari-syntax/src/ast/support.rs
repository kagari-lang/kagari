use crate::{
    ast::traits::AstNode,
    kind::SyntaxKind,
    syntax_node::{SyntaxNode, SyntaxToken},
};

pub fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

pub fn children<N: AstNode>(node: &SyntaxNode) -> impl Iterator<Item = N> {
    node.children().filter_map(N::cast)
}

pub fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .find_map(|element| match element {
            rowan::NodeOrToken::Token(token) if token.kind() == kind => Some(token),
            _ => None,
        })
}
