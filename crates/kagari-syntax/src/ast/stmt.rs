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

/// A block statement alternative, distinct from its final expression.
///
/// `val x = 1;` selects `BindingStmt`, `x = 2;` selects `AssignStmt`, and
/// `fn f() {}` selects `FnDef`. [`BlockExpr::tail`] stores a final expression;
/// `Error` retains damaged-source coverage.
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

node!(
    /// A local binding with a required pattern and optional type/initializer clauses.
    ///
    /// ```text
    /// var (x, y): Pair = incoming;
    /// binding = BindingKeyword(Var), pattern = Tuple(x, y)
    /// annotation = Some(TypeAnnotation(':', Pair)), initializer = Some(Initializer('=', incoming))
    /// terminator = ';' token
    /// ```
    ///
    /// Present required slots are abbreviated. A missing pattern cannot be replaced
    /// by the initializer. A written `=` with no value retains an [`Initializer`]
    /// with a missing expression, distinct from no initializer clause.
    BindingStmt {
        binding: Required<BindingKeyword>,
        pattern: Required<Pattern>,
        annotation: Option<Box<TypeAnnotation>>,
        initializer: Option<Initializer>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

group!(
    /// An equals token followed by a required binding initializer expression.
    ///
    /// In `val x = next;`, `equal` holds `=` and `expression` holds the `next`
    /// path. [`BindingStmt`] owns the pattern and semicolon.
    Initializer {
        pub equal: Token<tokens::Eq>,
        pub expression: Required<Expr>,
    }
);

node!(
    /// A return keyword, optional value expression and required semicolon.
    ///
    /// `return;` has `value = None`; `return x;` has `Some(PathExpr(x))`.
    /// `keyword` and `terminator` retain their tokens; missing punctuation is
    /// represented through [`Required`].
    ReturnStmt {
        keyword: Token<tokens::ReturnKw>,
        value: Option<Expr>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// An expression used as a statement, with context-dependent termination.
    ///
    /// ```text
    /// work();             expression = CallExpr(work), terminator = Some(Present(';'))
    /// if ready {} next(); first statement: terminator = None
    /// work() next();      first statement: terminator = Some(Missing)
    /// ```
    ///
    /// The expression's required wrapper is abbreviated. A block's final expression
    /// without a semicolon belongs to [`BlockExpr::tail`], not this statement node.
    ExprStmt {
        expression: Required<Expr>,
        terminator: Option<Required<Token<tokens::Semi>>>,
    }
);

/// A plain equals assignment or a compound binary operation.
///
/// `x = y` uses `Assign`; `x += y` uses `Compound(BinaryOp::Add)`.
/// The surrounding [`OperatorToken`] retains the actual assignment spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentOp {
    Assign,
    Compound(BinaryOp),
}

node!(
    /// A syntactic assignment target, parsed operator and required value.
    ///
    /// ```text
    /// x += next;
    /// target = PathExpr(x), operator.operation = Compound(Add), value = PathExpr(next)
    /// terminator = ';' token
    /// ```
    ///
    /// Present required slots are abbreviated. Target writeability and place
    /// classification belong to analysis; the parser retains the actual expression.
    AssignStmt {
        target: Required<Expr>,
        operator: Required<OperatorToken<AssignmentOp>>,
        value: Required<Expr>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// A while keyword with a condition and required block body.
    ///
    /// `while ready { work(); }` stores `condition = Expression(PathExpr(ready))`
    /// and a [`BlockExpr`] in `body`, abbreviating required wrappers.
    /// Binding conditions use [`Condition::Binding`]; condition and body are distinct roles.
    WhileStmt {
        keyword: Token<tokens::WhileKw>,
        condition: Required<Condition>,
        body: Required<BlockExpr>,
    }
);

node!(
    /// A loop statement with a keyword token and required block body.
    ///
    /// `loop { work(); }` stores `loop` in `keyword` and a [`BlockExpr`] in
    /// `body`. Expression-position loops use [`LoopExpr`](crate::ast::expr::LoopExpr).
    LoopStmt {
        keyword: Token<tokens::LoopKw>,
        body: Required<BlockExpr>,
    }
);

node!(
    /// A for-loop pattern, iterable expression and block with separate keywords.
    ///
    /// ```text
    /// for (x, y) in pairs { work(); }
    /// keyword = 'for', pattern = Tuple(x, y), in_keyword = 'in'
    /// iterable = PathExpr(pairs), body = BlockExpr
    /// ```
    ///
    /// Present required slots are abbreviated. Keywords are tokens; the pattern,
    /// iterable and body are independent children even during recovery.
    ForStmt {
        keyword: Token<tokens::ForKw>,
        pattern: Required<Pattern>,
        in_keyword: Required<Token<tokens::InKw>>,
        iterable: Required<Expr>,
        body: Required<BlockExpr>,
    }
);

node!(
    /// A break keyword with an optional value and required semicolon.
    ///
    /// `break;` has `value = None`; `break result;` has `Some(PathExpr(result))`.
    /// The parser retains this shape; the enclosing loop's value rules are checked later.
    BreakStmt {
        keyword: Token<tokens::BreakKw>,
        value: Option<Expr>,
        terminator: Required<Token<tokens::Semi>>,
    }
);

node!(
    /// A continue keyword and required semicolon, with no value field.
    ///
    /// `continue;` stores the `continue` token in `keyword` and the `;` token
    /// in `terminator`; a missing semicolon remains a required missing slot.
    ContinueStmt {
        keyword: Token<tokens::ContinueKw>,
        terminator: Required<Token<tokens::Semi>>,
    }
);
