//! Statements distinguish assignment targets, optional values and loop bodies.
//!
//! ```text
//! var (x, y): Pair = incoming;
//! BindingStmt { binding: Var, pattern: Tuple(x,y), annotation: Some(Pair),
//!               initializer: Some(incoming), terminator: semicolon, ... }
//! x += next;    AssignStmt { target: x, operator: Add, value: next, ... }
//! ```
//!
//! A binding's missing pattern remains a required missing slot; its initializer
//! cannot become the pattern. Assignment targets retain their written expression.

use crate::ast::{
    expr::{BinaryOp, BlockExpr, Condition, Expr, Pattern},
    item::FnDef,
    misc::{BindingKeyword, TypeAnnotation},
    site::{Required, SyntaxSite},
    tokens::{self, OperatorToken, Token},
};

#[derive(Debug)]
pub enum Stmt {
    BindingStmt(Box<BindingStmt>),
    ReturnStmt(Box<ReturnStmt>),
    ExprStmt(Box<ExprStmt>),
    AssignStmt(Box<AssignStmt>),
    WhileStmt(Box<WhileStmt>),
    LoopStmt(Box<LoopStmt>),
    ForStmt(Box<ForStmt>),
    BreakStmt(Box<BreakStmt>),
    ContinueStmt(Box<ContinueStmt>),
    FnDef(Box<FnDef>),
    Error(SyntaxSite),
}

node!(BindingStmt {
    binding: Required<BindingKeyword>,
    pattern: Required<Pattern>,
    annotation: Option<Box<TypeAnnotation>>,
    initializer: Option<Initializer>,
    terminator: Required<Token<tokens::Semi>>,
});

group!(Initializer {
    pub equal: Token<tokens::Eq>,
    pub expression: Required<Expr>,
});

node!(ReturnStmt {
    keyword: Token<tokens::ReturnKw>,
    value: Option<Expr>,
    terminator: Required<Token<tokens::Semi>>,
});

node!(ExprStmt {
    expression: Required<Expr>,
    terminator: Option<Required<Token<tokens::Semi>>>,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOp {
    Assign,
    Compound(BinaryOp),
}

node!(AssignStmt {
    target: Required<Expr>,
    operator: Required<OperatorToken<AssignmentOp>>,
    value: Required<Expr>,
    terminator: Required<Token<tokens::Semi>>,
});

node!(WhileStmt {
    keyword: Token<tokens::WhileKw>,
    condition: Required<Condition>,
    body: Required<BlockExpr>,
});

node!(LoopStmt {
    keyword: Token<tokens::LoopKw>,
    body: Required<BlockExpr>,
});

node!(ForStmt {
    keyword: Token<tokens::ForKw>,
    pattern: Required<Pattern>,
    in_keyword: Required<Token<tokens::InKw>>,
    iterable: Required<Expr>,
    body: Required<BlockExpr>,
});

node!(BreakStmt {
    keyword: Token<tokens::BreakKw>,
    value: Option<Expr>,
    terminator: Required<Token<tokens::Semi>>,
});

node!(ContinueStmt {
    keyword: Token<tokens::ContinueKw>,
    terminator: Required<Token<tokens::Semi>>,
});
