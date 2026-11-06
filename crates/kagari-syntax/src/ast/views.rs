//! Read-only alternatives derived from the stored enums; these never own nodes.

use kagari_common::span::Span;
use kagari_source::source::SourceFile;

use crate::ast::{
    borrow::{Ast, AstNode},
    expr::{
        ArrayExpr, AsyncBlockExpr, AwaitExpr, BinaryExpr, BlockExpr, CallExpr, CastExpr,
        ClosureExpr, Expr, FieldExpr, IfExpr, IndexExpr, InterpolatedString, Interpolation,
        Literal, LoopExpr, MatchExpr, ParenExpr, PathExpr, PrefixExpr, PropagateExpr, RangeExpr,
        StructExpr, TupleExpr,
    },
    item::{
        ConstDef, EnumDef, FnDef, ImplBlock, Item, ModuleDef, OpaqueTypeDef, StaticDef, StructDef,
        TraitDef, TypeAlias, UseDecl,
    },
    misc::Path,
    site::SyntaxSite,
    stmt::{
        AssignStmt, BindingStmt, BreakStmt, ContinueStmt, ExprStmt, ForStmt, LoopStmt, ReturnStmt,
        Stmt, WhileStmt,
    },
    tokens::{Bang, FormatText, Token},
    ty::{AppliedType, ArrayType, FunctionType, GroupedType, QualifiedType, TupleType, TypeSyntax},
    visit::NodeRef,
};

/// Ordered string fragments; literal decoding remains in `kagari-source`.
#[derive(Debug, Clone, Copy)]
pub enum StringPartView<'a> {
    Text(Ast<'a, Token<FormatText>>),
    Hole(Ast<'a, Interpolation>),
}

#[derive(Debug, Clone, Copy)]
pub enum PatternBoundView<'a> {
    Literal(Ast<'a, Literal>),
    Path(Ast<'a, PathExpr>),
}

macro_rules! borrowed_enum {
    ($stored:ident, $view:ident { $($variant:ident: $ty:ty),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy)]
        pub enum $view<'a> {
            $($variant(Ast<'a, $ty>),)+
            Error(Ast<'a, SyntaxSite>),
        }

        impl<'a> Ast<'a, $stored> {
            pub fn view(self) -> $view<'a> {
                match self.node {
                    $($stored::$variant(node) => $view::$variant(self.with::<$ty>(node)),)+
                    $stored::Error(site) => $view::Error(self.with(site)),
                }
            }
        }

        impl AstNode for $view<'_> {
            fn body_span(&self) -> Span { match self { $(Self::$variant(node) => node.body_span(),)+ Self::Error(node) => node.body_span() } }

            fn full_span(&self) -> Span { match self { $(Self::$variant(node) => node.full_span(),)+ Self::Error(node) => node.full_span() } }

            fn source(&self) -> &SourceFile { match self { $(Self::$variant(node) => node.source(),)+ Self::Error(node) => node.source() } }
        }

        view_nodes!($view { $($variant),+ });
    };
}

macro_rules! view_nodes {
    (TypeView { $($variant:ident),+ }) => {};
    ($view:ident { $($variant:ident),+ }) => {
        impl<'a> From<$view<'a>> for NodeRef<'a> {
            fn from(view: $view<'a>) -> Self {
                match view { $($view::$variant(node) => node.into(),)+ $view::Error(node) => node.into() }
            }
        }
    };
}

borrowed_enum!(
    Expr,
    ExprView {
        BlockExpr: BlockExpr,
        PathExpr: PathExpr,
        Literal: Literal,
        ParenExpr: ParenExpr,
        PrefixExpr: PrefixExpr,
        CastExpr: CastExpr,
        PropagateExpr: PropagateExpr,
        AwaitExpr: AwaitExpr,
        BinaryExpr: BinaryExpr,
        RangeExpr: RangeExpr,
        CallExpr: CallExpr,
        FieldExpr: FieldExpr,
        IndexExpr: IndexExpr,
        IfExpr: IfExpr,
        StructExpr: StructExpr,
        MatchExpr: MatchExpr,
        LoopExpr: LoopExpr,
        ClosureExpr: ClosureExpr,
        AsyncBlockExpr: AsyncBlockExpr,
        TupleExpr: TupleExpr,
        ArrayExpr: ArrayExpr,
        InterpolatedString: InterpolatedString,
    }
);

borrowed_enum!(
    Item,
    ItemView {
        ModuleDef: ModuleDef,
        UseDecl: UseDecl,
        FnDef: FnDef,
        ConstDef: ConstDef,
        StaticDef: StaticDef,
        StructDef: StructDef,
        EnumDef: EnumDef,
        TraitDef: TraitDef,
        ImplBlock: ImplBlock,
        TypeAlias: TypeAlias,
        OpaqueTypeDef: OpaqueTypeDef,
    }
);

borrowed_enum!(
    Stmt,
    StmtView {
        BindingStmt: BindingStmt,
        ReturnStmt: ReturnStmt,
        ExprStmt: ExprStmt,
        AssignStmt: AssignStmt,
        WhileStmt: WhileStmt,
        LoopStmt: LoopStmt,
        ForStmt: ForStmt,
        BreakStmt: BreakStmt,
        ContinueStmt: ContinueStmt,
        FnDef: FnDef,
    }
);

borrowed_enum!(TypeSyntax, TypeView {
    Named: Path, Applied: AppliedType, Never: Token<Bang>, Grouped: GroupedType,
    Tuple: TupleType, Array: ArrayType, Function: FunctionType, Qualified: QualifiedType,
});
