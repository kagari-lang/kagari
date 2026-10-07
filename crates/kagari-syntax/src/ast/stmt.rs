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
    /// The [`BindingStmt`] view; see its node diagram.
    BindingStmt(BindingStmt),
    /// The [`ReturnStmt`] view; see its node diagram.
    ReturnStmt(ReturnStmt),
    /// The [`AssignStmt`] view; see its node diagram.
    AssignStmt(AssignStmt),
    /// The [`WhileStmt`] view; see its node diagram.
    WhileStmt(WhileStmt),
    /// The [`LoopStmt`] view; see its node diagram.
    LoopStmt(LoopStmt),
    /// The [`ForStmt`] view; see its node diagram.
    ForStmt(ForStmt),
    /// The [`BreakStmt`] view; see its node diagram.
    BreakStmt(BreakStmt),
    /// The [`ContinueStmt`] view; see its node diagram.
    ContinueStmt(ContinueStmt),
    /// The [`ExprStmt`] view; see its node diagram.
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
    /// Reads direct `val`/`var` tokens; returns `None` if neither is present.
    pub fn writeability(&self) -> Option<Writeability> {
        if support::token(self.syntax(), SyntaxKind::ValKw).is_some() {
            Some(Writeability::Val)
        } else if support::token(self.syntax(), SyntaxKind::VarKw).is_some() {
            Some(Writeability::Var)
        } else {
            None
        }
    }

    /// Whether the direct binding keyword is `val`.
    pub fn is_val(&self) -> bool {
        self.writeability() == Some(Writeability::Val)
    }

    /// Whether the direct binding keyword is `var`.
    pub fn is_var(&self) -> bool {
        self.writeability() == Some(Writeability::Var)
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the type annotation or assigned type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }

    /// Returns the initializer: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn initializer(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl ReturnStmt {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl AssignStmt {
    /// Returns the first recognized direct operator token's kind.
    /// Returns `None` if no operator from this expression/statement family is present.
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

    /// Returns the assignment target: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn target(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the value expression: the second direct `Expr` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl WhileStmt {
    /// Returns the binding condition: the first direct `BindingCondition` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn binding_condition(&self) -> Option<BindingCondition> {
        support::child(self.syntax())
    }

    /// Returns the first direct expression, which is the body for a binding condition.
    /// Use `binding_condition()` to access a binding's nested initializer.
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }
}

impl LoopStmt {
    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }
}

impl BreakStmt {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl ForStmt {
    /// Returns the pattern: the first direct `Pattern` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }

    /// Returns the iterable: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn iterable(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ExprStmt {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}
