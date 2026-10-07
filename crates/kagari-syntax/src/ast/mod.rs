//! Typed views of the lossless concrete syntax tree.
//!
//! Every concrete wrapper stores just `syntax: SyntaxNode`. Logical components
//! such as a binary expression's left operand are selected from that node's
//! children on demand; there are no separately stored `lhs`/`rhs` fields.
//! Casting checks the node kind, without reparsing or validating its children.
//! Cloning a wrapper retains a handle to the same tree rather than deep-copying it.
//! Handles retain the tree independently of the original source and parse wrapper.
//!
//! # Reading the node diagrams
//!
//! Diagrams show direct child nesting in source order. A quoted spelling denotes
//! a token; unquoted kinds denote nodes. Where a node is followed by a source
//! example in parentheses, its descendants are collapsed. Trivia is omitted
//! unless stated otherwise. Accessor arrows identify the selected node/token.
//! Schemas mark optional children with `?`, repetitions with `*`, and alternatives
//! with `|`; these marks are notation, not additional nodes.
//!
//! [`SyntaxNode::children`](crate::syntax_node::SyntaxNode::children) skips tokens.
//! `filter_map(Expr::cast)` then keeps only expression nodes: `next()` selects
//! matching child #0, `nth(1)` matching child #1, `skip(1)` all matches after #0,
//! and `last()` the final match. These positions are not indices into all CST
//! elements. [`support::child`] finds the first matching *direct* node;
//! [`support::children`] returns all matching direct nodes in source order.
//!
//! # Recovery
//!
//! `Option` can mean legally omitted syntax or an incomplete recovered tree.
//! Iterators skip children of other kinds, including error nodes. Positional
//! accessors do not reconstruct missing slots. For example, in `if {}` the block
//! can be the first expression child, so `IfExpr::condition()` can select it.
//! Check binding conditions separately, and consult diagnostics before assuming
//! that a child's syntactic position establishes a valid language construct.

/// Generates a kind-checked node handle and forwards Rustdoc to its public type.
macro_rules! ast_node {
    ($(#[$meta:meta])* $name:ident, $kind:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub struct $name {
            syntax: $crate::syntax_node::SyntaxNode,
        }

        impl $crate::ast::traits::AstNode for $name {
            fn can_cast(kind: $crate::kind::SyntaxKind) -> bool {
                kind == $crate::kind::SyntaxKind::$kind
            }

            fn cast(syntax: $crate::syntax_node::SyntaxNode) -> Option<Self> {
                Self::can_cast(syntax.kind()).then_some(Self { syntax })
            }

            fn syntax(&self) -> &$crate::syntax_node::SyntaxNode {
                &self.syntax
            }
        }
    };
}

pub mod expr;
pub mod item;
pub mod misc;
pub mod stmt;
pub mod support;
pub mod traits;
pub mod ty;
