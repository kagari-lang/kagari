//! Statement views inside [`BlockExpr`].
//!
//! [`Stmt`] classifies existing statement nodes. Block tails remain direct
//! expressions rather than [`ExprStmt`] nodes. See [`crate::ast`] for tree notation
//! and positional access in recovered syntax.

use crate::{
    ast::{
        expr::{BindingCondition, BlockExpr, Expr, Pattern},
        misc::{Name, Writeability},
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

ast_node!(
    /// A `val` or `var` local declaration with optional type and initializer.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `var count: i32 = 1;`:
    ///
    /// ```text
    /// BindingStmt
    /// +-- VarKw "var" [token]
    /// +-- Name (count) [node] -> name()
    /// +-- Colon ":" [token]
    /// +-- TypeRef (i32) [node] -> ty()
    /// +-- Eq "=" [token]
    /// +-- Literal (1) [node] [Expr #0] -> initializer()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// Type and initializer can be absent in the syntax tree. `writeability()` reads
    /// `val`/`var` tokens; an absent keyword after recovery yields `None`.
    BindingStmt,
    BindingStmt
);

ast_node!(
    /// A `return` statement with an optional result expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `return 1;`:
    ///
    /// ```text
    /// ReturnStmt
    /// +-- ReturnKw "return" [token]
    /// +-- Literal (1) [node] [Expr #0] -> expr()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// `return;` has no expression child and `expr()` returns `None`.
    ReturnStmt,
    ReturnStmt
);

ast_node!(
    /// An assignment or compound assignment to a syntactic target.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `count += 1;`:
    ///
    /// ```text
    /// AssignStmt
    /// +-- PathExpr (count) [node] [Expr #0] -> target()
    /// +-- PlusEq "+=" [token] -> operator()
    /// +-- Literal (1) [node] [Expr #1] -> value()
    /// `-- Semi ";" [token]
    /// ```
    AssignStmt,
    AssignStmt
);

ast_node!(
    /// A while loop with an expression or binding condition.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `while ready { continue; }`:
    ///
    /// ```text
    /// WhileStmt
    /// +-- WhileKw "while" [token]
    /// +-- PathExpr (ready) [node] [Expr #0] -> condition()
    /// `-- BlockExpr ({ continue; }) [node] [Expr #1] -> body()
    /// ```
    ///
    /// `while val Some(x) = value { consume(x); }`:
    ///
    /// ```text
    /// WhileStmt
    /// +-- WhileKw "while" [token]
    /// +-- BindingCondition (val Some(x) = value) [node] -> binding_condition()
    /// `-- BlockExpr ({ consume(x); }) [node] [Expr #0] -> condition(), body()
    /// ```
    ///
    /// As with [`IfExpr`](crate::ast::expr::IfExpr), inspect `binding_condition()` first.
    /// For a binding condition, the first direct `Expr` is the body block, so
    /// `condition()` does not return the initializer nested in `BindingCondition`.
    WhileStmt,
    WhileStmt
);

ast_node!(
    /// A `loop` parsed directly as a block statement.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `loop { break; }`:
    ///
    /// ```text
    /// LoopStmt
    /// +-- LoopKw "loop" [token]
    /// `-- BlockExpr ({ break; }) [node] [Expr #0] -> body()
    /// ```
    LoopStmt,
    LoopStmt
);

ast_node!(
    /// A pattern binding, iterable expression and loop body.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `for x in values { consume(x); }`:
    ///
    /// ```text
    /// ForStmt
    /// +-- ForKw "for" [token]
    /// +-- Pattern (x) [node] -> pattern()
    /// +-- InKw "in" [token]
    /// +-- PathExpr (values) [node] [Expr #0] -> iterable()
    /// `-- BlockExpr ({ consume(x); }) [node] [Expr #1] -> body()
    /// ```
    ///
    /// The iterable is the first direct expression and the body is the first direct
    /// block. Recovery can leave the body as the only expression; accessors do not
    /// reconstruct a missing iterable.
    ForStmt,
    ForStmt
);

ast_node!(
    /// A `break` statement with an optional loop result expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `break 1;`:
    ///
    /// ```text
    /// BreakStmt
    /// +-- BreakKw "break" [token]
    /// +-- Literal (1) [node] [Expr #0] -> expr()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// `break;` has no expression; whether a value is allowed is checked later.
    BreakStmt,
    BreakStmt
);

ast_node!(
    /// A `continue;` statement with no value child.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `continue;`:
    ///
    /// ```text
    /// ContinueStmt
    /// +-- ContinueKw "continue" [token]
    /// `-- Semi ";" [token]
    /// ```
    ContinueStmt,
    ContinueStmt
);

ast_node!(
    /// An expression consumed as a statement rather than a block tail.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `consume(1);`:
    ///
    /// ```text
    /// ExprStmt
    /// +-- CallExpr (consume(1)) [node] [Expr #0] -> expr()
    /// `-- Semi ";" [token]
    /// ```
    ///
    /// A trailing block value is a direct expression instead of this wrapper.
    /// A block or `if` used before another statement can omit the semicolon.
    ExprStmt,
    ExprStmt
);

/// A typed choice of statement views; see each variant for its tree shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stmt {
    /// A local binding, such as `val x = 1;` or `var count: i32 = 0;`.
    BindingStmt(BindingStmt),
    /// A return with an optional value: `return value;` or `return;`.
    ReturnStmt(ReturnStmt),
    /// An assignment, such as `x = value;` or `values[i] += delta;`.
    AssignStmt(AssignStmt),
    /// A conditional loop, such as `while ready { tick(); }` or `while val Some(x) = next() {}`.
    WhileStmt(WhileStmt),
    /// A loop in statement position, such as `loop { tick(); }`.
    LoopStmt(LoopStmt),
    /// Iteration with a pattern, such as `for (key, value) in entries { use_value(value); }`.
    ForStmt(ForStmt),
    /// A loop exit with an optional value: `break;` or `break 42;`.
    BreakStmt(BreakStmt),
    /// A skip to the next loop iteration: `continue;`.
    ContinueStmt(ContinueStmt),
    /// An expression used as a statement, such as `work();`, rather than a block's tail value.
    ExprStmt(ExprStmt),
}

impl AstNode for Stmt {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::BindingStmt
                | SyntaxKind::ReturnStmt
                | SyntaxKind::AssignStmt
                | SyntaxKind::WhileStmt
                | SyntaxKind::LoopStmt
                | SyntaxKind::ForStmt
                | SyntaxKind::BreakStmt
                | SyntaxKind::ContinueStmt
                | SyntaxKind::ExprStmt
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::BindingStmt => BindingStmt::cast(syntax).map(Self::BindingStmt),
            SyntaxKind::ReturnStmt => ReturnStmt::cast(syntax).map(Self::ReturnStmt),
            SyntaxKind::AssignStmt => AssignStmt::cast(syntax).map(Self::AssignStmt),
            SyntaxKind::WhileStmt => WhileStmt::cast(syntax).map(Self::WhileStmt),
            SyntaxKind::LoopStmt => LoopStmt::cast(syntax).map(Self::LoopStmt),
            SyntaxKind::ForStmt => ForStmt::cast(syntax).map(Self::ForStmt),
            SyntaxKind::BreakStmt => BreakStmt::cast(syntax).map(Self::BreakStmt),
            SyntaxKind::ContinueStmt => ContinueStmt::cast(syntax).map(Self::ContinueStmt),
            SyntaxKind::ExprStmt => ExprStmt::cast(syntax).map(Self::ExprStmt),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::BindingStmt(node) => node.syntax(),
            Self::ReturnStmt(node) => node.syntax(),
            Self::AssignStmt(node) => node.syntax(),
            Self::WhileStmt(node) => node.syntax(),
            Self::LoopStmt(node) => node.syntax(),
            Self::ForStmt(node) => node.syntax(),
            Self::BreakStmt(node) => node.syntax(),
            Self::ContinueStmt(node) => node.syntax(),
            Self::ExprStmt(node) => node.syntax(),
        }
    }
}

impl BindingStmt {
    /// Returns `Some(Writeability::Val)` for `val x = 1;` and `Some(Writeability::Var)` for `var x = 1;`.
    /// Returns `None` if neither direct keyword is present.
    pub fn writeability(&self) -> Option<Writeability> {
        if support::token(self.syntax(), SyntaxKind::ValKw).is_some() {
            Some(Writeability::Val)
        } else if support::token(self.syntax(), SyntaxKind::VarKw).is_some() {
            Some(Writeability::Var)
        } else {
            None
        }
    }

    /// Whether this is a `val` binding: true for `val x = 1;`, false for `var x = 1;`.
    pub fn is_val(&self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    /// Whether this is a `var` binding: true for `var x = 1;`, false for `val x = 1;`.
    pub fn is_var(&self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }

    /// Returns `count` in `val count: i32 = 1;`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `Some("count")` for `val count = 1;`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the annotation `i32` in `val count: i32 = 1;`.
    /// Returns `None` for `val count = 1;`; this accessor does not infer a type.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns `1 + 2` in `val count = 1 + 2;`.
    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl ReturnStmt {
    /// Returns `value` in `return value;`; returns `None` for `return;`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl AssignStmt {
    /// Returns `Some(SyntaxKind::Eq)` for `x = y;` and `Some(SyntaxKind::PlusEq)` for `x += y;`.
    /// Returns `None` if no direct assignment operator remains.
    pub fn operator(&self) -> Option<SyntaxKind> {
        self.syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .map(|token| token.kind())
            .find(|kind| {
                matches!(
                    kind,
                    SyntaxKind::Eq
                        | SyntaxKind::PlusEq
                        | SyntaxKind::MinusEq
                        | SyntaxKind::StarEq
                        | SyntaxKind::SlashEq
                        | SyntaxKind::PercentEq
                        | SyntaxKind::AmpEq
                        | SyntaxKind::PipeEq
                        | SyntaxKind::CaretEq
                        | SyntaxKind::ShlEq
                        | SyntaxKind::ShrEq
                )
            })
    }

    /// Returns `values[i]` in `values[i] += delta;`.
    /// Selects the first direct expression without descending into the target.
    pub fn target(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns `delta` in `values[i] += delta;`.
    /// Selects the second direct expression, or `None` if recovery leaves fewer than two.
    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl WhileStmt {
    /// Returns `val Some(x) = next()` in `while val Some(x) = next() { use_value(x); }`.
    /// Returns `None` for `while ready { work(); }`.
    pub fn binding_condition(&self) -> Option<BindingCondition> {
        support::child(self.syntax())
    }

    /// Returns `ready` in `while ready { work(); }`.
    /// For `while val Some(x) = next() { use_value(x); }`, returns the body block instead.
    /// Check `binding_condition()` first to access its nested initializer `next()`.
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns `{ work(); }` in `while ready { work(); }`, including for binding conditions.
    pub fn body(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }
}

impl LoopStmt {
    /// Returns `{ tick(); }` in the statement `loop { tick(); }`.
    pub fn body(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }
}

impl BreakStmt {
    /// Returns `42` in `break 42;`; returns `None` for `break;`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl ForStmt {
    /// Returns `(key, value)` in `for (key, value) in entries { use_value(value); }`.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }

    /// Returns `entries` in `for item in entries { use_value(item); }`.
    pub fn iterable(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns `{ use_value(item); }` in `for item in entries { use_value(item); }`.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ExprStmt {
    /// Returns `work()` in `work();`, without the terminating semicolon.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}
