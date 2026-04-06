use crate::{
    ast::{expr::Expr, macros::ast_node, misc::Name, support, traits::AstNode},
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(LetStmt, LetStmt);
ast_node!(ReturnStmt, ReturnStmt);
ast_node!(AssignStmt, AssignStmt);
ast_node!(WhileStmt, WhileStmt);
ast_node!(LoopStmt, LoopStmt);
ast_node!(BreakStmt, BreakStmt);
ast_node!(ContinueStmt, ContinueStmt);
ast_node!(ExprStmt, ExprStmt);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    LetStmt(LetStmt),
    ReturnStmt(ReturnStmt),
    AssignStmt(AssignStmt),
    WhileStmt(WhileStmt),
    LoopStmt(LoopStmt),
    BreakStmt(BreakStmt),
    ContinueStmt(ContinueStmt),
    ExprStmt(ExprStmt),
}

impl AstNode for Stmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::LetStmt
                | SyntaxKind::ReturnStmt
                | SyntaxKind::AssignStmt
                | SyntaxKind::WhileStmt
                | SyntaxKind::LoopStmt
                | SyntaxKind::BreakStmt
                | SyntaxKind::ContinueStmt
                | SyntaxKind::ExprStmt
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::LetStmt => LetStmt::cast(syntax).map(Self::LetStmt),
            SyntaxKind::ReturnStmt => ReturnStmt::cast(syntax).map(Self::ReturnStmt),
            SyntaxKind::AssignStmt => AssignStmt::cast(syntax).map(Self::AssignStmt),
            SyntaxKind::WhileStmt => WhileStmt::cast(syntax).map(Self::WhileStmt),
            SyntaxKind::LoopStmt => LoopStmt::cast(syntax).map(Self::LoopStmt),
            SyntaxKind::BreakStmt => BreakStmt::cast(syntax).map(Self::BreakStmt),
            SyntaxKind::ContinueStmt => ContinueStmt::cast(syntax).map(Self::ContinueStmt),
            SyntaxKind::ExprStmt => ExprStmt::cast(syntax).map(Self::ExprStmt),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::LetStmt(node) => node.syntax(),
            Self::ReturnStmt(node) => node.syntax(),
            Self::AssignStmt(node) => node.syntax(),
            Self::WhileStmt(node) => node.syntax(),
            Self::LoopStmt(node) => node.syntax(),
            Self::BreakStmt(node) => node.syntax(),
            Self::ContinueStmt(node) => node.syntax(),
            Self::ExprStmt(node) => node.syntax(),
        }
    }
}

impl LetStmt {
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    pub fn ty(&self) -> Option<crate::ast::ty::TypeRef> {
        support::child(self.syntax())
    }

    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl ReturnStmt {
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl AssignStmt {
    pub fn target(&self) -> Option<crate::ast::expr::PathExpr> {
        support::child(self.syntax())
    }

    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl WhileStmt {
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    pub fn body(&self) -> Option<crate::ast::expr::BlockExpr> {
        self.syntax()
            .children()
            .filter_map(crate::ast::expr::BlockExpr::cast)
            .next()
    }
}

impl LoopStmt {
    pub fn body(&self) -> Option<crate::ast::expr::BlockExpr> {
        self.syntax()
            .children()
            .filter_map(crate::ast::expr::BlockExpr::cast)
            .next()
    }
}

impl ExprStmt {
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}
