mod expr;
mod item;
mod misc;
mod stmt;
pub mod support;
pub mod traits;
mod ty;

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

pub use expr::{
    BindingCondition, BlockExpr, Expr, Literal, MatchArm, MatchArmList, PathExpr, Pattern,
    PatternBound,
};
pub use item::{
    AssociatedType, Attribute, AttributeArg, AttributeArgs, AttributeValue, ConstDef, EnumDef,
    FnDef, ImplBlock, Item, MethodDef, ModuleBlock, ModuleDef, SourceFile, StructDef, TraitDef,
    UseDecl, UseTree, UseTreeList, Visibility,
};
pub use misc::{
    Field, FieldList, GenericArgList, GenericParam, GenericParamList, Name, Param, ParamList, Path,
    TraitBoundList, TraitRef, TypeList, Variant, VariantList, WhereClause, WherePredicate,
    Writeability,
};
pub use stmt::{AssignStmt, BindingStmt, ExprStmt, ReturnStmt, Stmt};
pub use traits::AstNode;
pub use ty::{ArrayType, FunctionType, TupleType, TypeRef};

pub use expr::{InterpolatedString, Interpolation};
