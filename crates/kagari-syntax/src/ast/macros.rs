macro_rules! ast_node {
    ($name:ident, $kind:ident) => {
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

pub(crate) use ast_node;
