//! Selection primitives used by the typed AST accessors.
//!
//! These functions inspect direct children, never recursively search descendants.
//! Whitespace and punctuation tokens do not participate in node selection. See
//! the [AST diagram conventions](crate::ast) for filtered child positions.

use crate::{
    ast::traits::AstNode,
    kind::SyntaxKind,
    syntax_node::{SyntaxNode, SyntaxToken},
};
use rowan::NodeOrToken;

/// Returns the first direct child that casts to `N`, or `None` if none matches.
pub fn child<N: AstNode>(node: &SyntaxNode) -> Option<N> {
    node.children().find_map(N::cast)
}

/// Iterates matching direct child nodes in source order; the iterator may be empty.
pub fn children<N: AstNode>(node: &SyntaxNode) -> impl Iterator<Item = N> {
    node.children().filter_map(N::cast)
}

/// Returns the first direct token of `kind`, without inspecting nested nodes.
pub fn token(node: &SyntaxNode, kind: SyntaxKind) -> Option<SyntaxToken> {
    node.children_with_tokens()
        .find_map(|element| match element {
            NodeOrToken::Token(token) if token.kind() == kind => Some(token),
            _ => None,
        })
}
