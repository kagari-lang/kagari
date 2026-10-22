//! Expression and pattern views over the CST.
//!
//! Each concrete type shows its actual children; [`Expr`] groups expression kinds
//! without adding another tree node. See [`crate::ast`] for traversal and recovery
//! conventions. Operators are syntax tokens; evaluation and typing happen later.

use crate::{
    ast::{
        misc::{GenericArgList, Name, Path},
        stmt::Stmt,
        support,
        traits::AstNode,
        ty::TypeRef,
    },
    kind::SyntaxKind,
    syntax_node::SyntaxNode,
};

use rowan::NodeOrToken;

ast_node!(
    /// A braced sequence of statements with an optional trailing expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `{ val x = 1; x + 2 }`:
    ///
    /// ```text
    /// BlockExpr
    /// +-- LBrace "{" [token]
    /// +-- BindingStmt (val x = 1;) [node] -> statements()
    /// +-- BinaryExpr (x + 2) [node] [Expr #0] -> tail_expr()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// Statements are wrapped nodes; the tail is a direct expression without an `ExprStmt` wrapper.
    /// `statements()` filters to `Stmt`; `tail_expr()` takes the last direct `Expr`,
    /// returning `None` for a block containing only statements.
    BlockExpr,
    BlockExpr
);

ast_node!(
    /// An unresolved expression path or qualified member spelling.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `math::total`:
    ///
    /// ```text
    /// PathExpr
    /// `-- Path (math::total) [node] -> path()
    /// ```
    ///
    /// `Box<i32>::make`:
    ///
    /// ```text
    /// PathExpr
    /// +-- Path (Box) [node] -> path()
    /// +-- GenericArgList (<i32>) [node] -> generic_args()
    /// +-- ColonColon "::" [token]
    /// `-- Name (make) [node] -> name()
    /// ```
    ///
    /// `identity::<i32>` as a function value:
    ///
    /// ```text
    /// PathExpr
    /// +-- Path (identity) [node] -> path()
    /// +-- ColonColon "::" [token]
    /// `-- GenericArgList (<i32>) [node] -> generic_args()
    /// ```
    ///
    /// `<T as Read>::make`:
    ///
    /// ```text
    /// PathExpr
    /// `-- TypeRef (<T as Read>::make) [node] -> qualified_type()
    /// ```
    ///
    /// The plain form contains `Path`; `Box<i32>::make` adds direct generic arguments
    /// and a member `Name`. The fully qualified form contains `TypeRef -> QualifiedType`.
    /// `name_text()` omits generic arguments; for a qualified form it returns only the
    /// member name. These are source spellings, not resolved declarations.
    PathExpr,
    PathExpr
);

ast_node!(
    /// A literal token, retaining its spelling rather than a decoded value.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `42`:
    ///
    /// ```text
    /// Literal
    /// `-- Number "42" [token] -> text(), kind()
    /// ```
    ///
    /// `text()` and `kind()` read the first token; they do not decode or type-check it.
    Literal,
    Literal
);

ast_node!(
    /// A parenthesized expression that preserves explicit grouping.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `(1 + 2)`:
    ///
    /// ```text
    /// ParenExpr
    /// +-- LParen "(" [token]
    /// +-- BinaryExpr (1 + 2) [node] [Expr #0] -> expr()
    /// `-- RParen ")" [token]
    /// ```
    ParenExpr,
    ParenExpr
);

ast_node!(
    /// A unary `-` or `!` expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `-1`:
    ///
    /// ```text
    /// PrefixExpr
    /// +-- Minus "-" [token] -> operator()
    /// `-- Literal (1) [node] [Expr #0] -> expr()
    /// ```
    PrefixExpr,
    PrefixExpr
);

ast_node!(
    /// An `as` expression with a target type spelling.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `1 as i64`:
    ///
    /// ```text
    /// CastExpr
    /// +-- Literal (1) [node] [Expr #0] -> expr()
    /// +-- AsKw "as" [token]
    /// `-- TypeRef (i64) [node] -> ty()
    /// ```
    CastExpr,
    CastExpr
);

ast_node!(
    /// A postfix `.await` expression retaining its operand as a direct child.
    ///
    /// `read().await` contains a `CallExpr`, a dot and the `await` keyword.
    /// Semantic checking determines the permitted context and result type.
    AwaitExpr,
    AwaitExpr
);

ast_node!(
    /// A postfix `?` expression; propagation semantics are checked in HIR.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `read()?`:
    ///
    /// ```text
    /// PropagateExpr
    /// +-- CallExpr (read()) [node] [Expr #0] -> expr()
    /// `-- Question "?" [token]
    /// ```
    PropagateExpr,
    PropagateExpr
);

ast_node!(
    /// Two operands joined by an arithmetic, comparison, bitwise or logical operator.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `total + 1`:
    ///
    /// ```text
    /// BinaryExpr
    /// +-- PathExpr (total) [node] [Expr #0] -> lhs()
    /// +-- Plus "+" [token] -> operator()
    /// `-- Literal (1) [node] [Expr #1] -> rhs()
    /// ```
    ///
    /// `total +` (recovered syntax with diagnostics):
    ///
    /// ```text
    /// BinaryExpr
    /// +-- PathExpr (total) [node] [Expr #0] -> lhs()
    /// `-- Plus "+" [token] -> operator()
    /// ```
    BinaryExpr,
    BinaryExpr
);

ast_node!(
    /// An exclusive `..` or inclusive `..=` range with optional endpoints.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `1..=3`:
    ///
    /// ```text
    /// RangeExpr
    /// +-- Literal (1) [node] [Expr #0] -> start()
    /// +-- DotDotEq "..=" [token]
    /// `-- Literal (3) [node] [Expr #1] -> end()
    /// ```
    ///
    /// `..3`:
    ///
    /// ```text
    /// RangeExpr
    /// +-- DotDot ".." [token]
    /// `-- Literal (3) [node] [Expr #0] -> end()
    /// ```
    ///
    /// `1..`:
    ///
    /// ```text
    /// RangeExpr
    /// +-- Literal (1) [node] [Expr #0] -> start()
    /// `-- DotDot ".." [token]
    /// ```
    ///
    /// `..`:
    ///
    /// ```text
    /// RangeExpr
    /// `-- DotDot ".." [token]
    /// ```
    ///
    /// Endpoints are selected relative to the `..`/`..=` token, not by fixed expression
    /// indices. An exclusive range can omit either endpoint; an inclusive range requires
    /// an end in valid syntax. Missing required parts remain possible after recovery.
    RangeExpr,
    RangeExpr
);

ast_node!(
    /// A callee followed by arguments, with an optional generic argument list.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `sum(1, 2)`:
    ///
    /// ```text
    /// CallExpr
    /// +-- PathExpr (sum) [node] [Expr #0] -> callee()
    /// +-- LParen "(" [token]
    /// +-- Literal (1) [node] [Expr #1] -> args()
    /// +-- Comma "," [token]
    /// +-- Literal (2) [node] [Expr #2] -> args()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `sum::<i32>(1, 2)`:
    ///
    /// ```text
    /// CallExpr
    /// +-- PathExpr (sum) [node] [Expr #0] -> callee()
    /// +-- ColonColon "::" [token]
    /// +-- GenericArgList (<i32>) [node] -> generic_args()
    /// +-- LParen "(" [token]
    /// +-- Literal (1) [node] [Expr #1] -> args()
    /// +-- Comma "," [token]
    /// +-- Literal (2) [node] [Expr #2] -> args()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// The filtered expression sequence starts with the callee. `args()` uses `skip(1)`
    /// on that sequence; punctuation and generic arguments do not occupy expression
    /// positions. There is no separate argument-list node.
    CallExpr,
    CallExpr
);

ast_node!(
    /// A named member access on a receiver expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `point.x`:
    ///
    /// ```text
    /// FieldExpr
    /// +-- PathExpr (point) [node] [Expr #0] -> receiver()
    /// +-- Dot "." [token]
    /// `-- Name (x) [node] -> name()
    /// ```
    FieldExpr,
    FieldExpr
);

ast_node!(
    /// An indexed access with a receiver and index expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `values[0]`:
    ///
    /// ```text
    /// IndexExpr
    /// +-- PathExpr (values) [node] [Expr #0] -> receiver()
    /// +-- LBracket "[" [token]
    /// +-- Literal (0) [node] [Expr #1] -> index()
    /// `-- RBracket "]" [token]
    /// ```
    IndexExpr,
    IndexExpr
);

ast_node!(
    /// A conditional with an expression or binding condition and optional else branch.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `if ready { 1 } else { 2 }`:
    ///
    /// ```text
    /// IfExpr
    /// +-- IfKw "if" [token]
    /// +-- PathExpr (ready) [node] [Expr #0] -> condition()
    /// +-- BlockExpr ({ 1 }) [node] [Expr #1] -> then_branch()
    /// +-- ElseKw "else" [token]
    /// `-- BlockExpr ({ 2 }) [node] [Expr #2] -> else_branch()
    /// ```
    ///
    /// `if val Some(x) = value { x } else if ready { 1 } else { 2 }`:
    ///
    /// ```text
    /// IfExpr
    /// +-- IfKw "if" [token]
    /// +-- BindingCondition (val Some(x) = value) [node] -> binding_condition()
    /// +-- BlockExpr ({ x }) [node] [Expr #0] -> condition(), then_branch()
    /// +-- ElseKw "else" [token]
    /// `-- IfExpr (if ready { 1 } else { 2 }) [node] [Expr #1] -> else_branch()
    /// ```
    ///
    /// `if {}` (recovered syntax with diagnostics):
    ///
    /// ```text
    /// IfExpr
    /// +-- IfKw "if" [token]
    /// +-- BlockExpr ({}) [node] [Expr #0] -> condition(), then_branch()
    /// `-- BlockExpr () [node] [Expr #1] -> else_branch()
    /// ```
    ///
    /// Check `binding_condition()` before using `condition()`: `BindingCondition` is
    /// not an `Expr`, while `BlockExpr` is. For a binding condition, `condition()` selects
    /// the then block. `then_branch()` selects the first direct block; `else_branch()`
    /// selects the first expression after that block. Recovery can change these roles: in
    /// `if {}`, a condition block and an empty recovered body are both block children.
    IfExpr,
    IfExpr
);

ast_node!(
    /// A constructor path, explicit field initializers and an optional terminal update base.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Point { x: 1, y, ..base }`:
    ///
    /// ```text
    /// StructExpr
    /// +-- PathExpr (Point) [node] [Expr #0] -> path()
    /// +-- LBrace "{" [token]
    /// +-- FieldInitList (x: 1, y,) [node] -> field_list()
    /// +-- StructUpdateBase (..base) [node] -> update_base()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// Braces are direct tokens here; `FieldInitList` owns explicit fields and their
    /// commas, including the separator before the base. Its list is empty in
    /// `Point { ..base }`. Generic constructor arguments, when present, are direct
    /// children before the opening brace. `StructUpdateBase` is not an `Expr`, so
    /// filtering direct children by `Expr::cast` selects only the constructor path,
    /// not the nested base operand. An absent marker yields `update_base() == None`;
    /// recovered `Point { .. }` retains a base node whose `value()` is `None`.
    StructExpr,
    StructExpr
);

ast_node!(
    /// The explicit field initializer list of a struct expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: 1, y: 2`:
    ///
    /// ```text
    /// FieldInitList
    /// +-- FieldInit (x: 1) [node] -> fields()
    /// +-- Comma "," [token]
    /// `-- FieldInit (y: 2) [node] -> fields()
    /// ```
    ///
    /// Iterates only direct `FieldInit` children in source order; commas are tokens.
    /// The enclosing braces and optional `StructUpdateBase` belong to `StructExpr`,
    /// not this list. The list is present but empty for `Point {}` and `Point { ..base }`.
    FieldInitList,
    FieldInitList
);

ast_node!(
    /// A struct update marker with its expression operand, not a standalone expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// In `Point { x: 1, ..make_point() }`, the direct `StructExpr` child is:
    ///
    /// ```text
    /// StructUpdateBase
    /// +-- DotDot ".." [token]
    /// `-- CallExpr (make_point()) [node] [Expr #0] -> value()
    /// ```
    ///
    /// Trivia is omitted and the call's descendants are collapsed. `value()` selects
    /// the first direct expression node, not physical child/token #0. The marker
    /// is not a range expression or a pattern rest token. A base is terminal and
    /// permits no trailing comma. Recovered `Point { .. }` retains `DotDot` but no
    /// expression child; `value()` returns `None` without hiding marker presence.
    /// An invalid `..=` marker is retained in an error node instead. Repeated bases
    /// and post-base fields are retained under error nodes, outside the explicit list.
    StructUpdateBase,
    StructUpdateBase
);

ast_node!(
    /// One named field initializer, with an optional explicit value.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: 1`:
    ///
    /// ```text
    /// FieldInit
    /// +-- Name (x) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- Literal (1) [node] [Expr #0] -> value()
    /// ```
    ///
    /// `x`:
    ///
    /// ```text
    /// FieldInit
    /// `-- Name (x) [node] -> name()
    /// ```
    ///
    /// A shorthand initializer such as `Point { x }` has only `Name`; `value()` returns
    /// `None`. Lowering decides how to interpret shorthand.
    FieldInit,
    FieldInit
);

ast_node!(
    /// A scrutinee followed by a list of pattern arms.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `match value { 0 => 1, _ => 2 }`:
    ///
    /// ```text
    /// MatchExpr
    /// +-- MatchKw "match" [token]
    /// +-- PathExpr (value) [node] [Expr #0] -> scrutinee()
    /// +-- LBrace "{" [token]
    /// +-- MatchArmList (0 => 1, _ => 2) [node] -> arms()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// The scrutinee is the first direct expression; arm expressions are nested below
    /// `MatchArmList` and do not enter that selection.
    MatchExpr,
    MatchExpr
);

ast_node!(
    /// A `loop` used in expression position, such as a binding initializer.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `loop { break 1; }`:
    ///
    /// ```text
    /// LoopExpr
    /// +-- LoopKw "loop" [token]
    /// `-- BlockExpr ({ break 1; }) [node] [Expr #0] -> body()
    /// ```
    LoopExpr,
    LoopExpr
);

ast_node!(
    /// A cold future expression with its own return and suspension boundary.
    ///
    /// ```text
    /// AsyncBlockExpr
    /// +-- AsyncKw "async" [token]
    /// `-- BlockExpr { fetch().await } [node] -> body()
    /// ```
    ///
    /// The braces belong to the child block. `body()` is absent when recovery
    /// retains an `async` keyword without an opening brace; an incomplete block
    /// remains available with its successfully parsed children.
    AsyncBlockExpr,
    AsyncBlockExpr
);

ast_node!(
    /// A closure parameter list followed by its expression body.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `|x: i32, y| x + y`:
    ///
    /// ```text
    /// ClosureExpr
    /// +-- Pipe "|" [token]
    /// +-- ClosureParamList (x: i32, y) [node] -> params() via list
    /// +-- Pipe "|" [token]
    /// `-- BinaryExpr (x + y) [node] [Expr #0] -> body()
    /// ```
    ///
    /// `|| 1`:
    ///
    /// ```text
    /// ClosureExpr
    /// +-- PipePipe "||" [token]
    /// `-- Literal (1) [node] [Expr #0] -> body()
    /// ```
    ///
    /// `params()` traverses through `ClosureParamList`; `body()` takes the last direct
    /// expression. The `||` form has no parameter-list node and yields an empty iterator.
    ClosureExpr,
    ClosureExpr
);

ast_node!(
    /// The parameters between a closure's pipes.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: i32, y`:
    ///
    /// ```text
    /// ClosureParamList
    /// +-- ClosureParam (x: i32) [node]
    /// +-- Comma "," [token]
    /// `-- ClosureParam (y) [node]
    /// ```
    ///
    /// Pipes belong to `ClosureExpr`. Read parameter nodes through
    /// [`ClosureExpr::params`], which preserves their order.
    ClosureParamList,
    ClosureParamList
);

ast_node!(
    /// A closure parameter with an optional type annotation.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: i32`:
    ///
    /// ```text
    /// ClosureParam
    /// +-- Name (x) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- TypeRef (i32) [node] -> ty()
    /// ```
    ///
    /// An omitted type annotation is legal; `ty()` then returns `None`.
    ClosureParam,
    ClosureParam
);

ast_node!(
    /// The list of match arms; enclosing braces belong to the match expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `0 => 1, _ => 2`:
    ///
    /// ```text
    /// MatchArmList
    /// +-- MatchArm (0 => 1) [node] -> arms()
    /// +-- Comma "," [token]
    /// `-- MatchArm (_ => 2) [node] -> arms()
    /// ```
    MatchArmList,
    MatchArmList
);

ast_node!(
    /// A pattern, optional `if` guard, and result expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x if ready => x`:
    ///
    /// ```text
    /// MatchArm
    /// +-- Pattern (x) [node] -> pattern()
    /// +-- IfKw "if" [token]
    /// +-- PathExpr (ready) [node] [Expr #0] -> guard()
    /// +-- FatArrow "=>" [token]
    /// `-- PathExpr (x) [node] [Expr #1] -> expr()
    /// ```
    ///
    /// With a guard, the first direct expression is the guard and the last is the arm
    /// result. Without `if`, `guard()` returns `None`. Recovery may leave only one
    /// expression, in which case both positional accessors can select the same node.
    MatchArm,
    MatchArm
);

ast_node!(
    /// A `val pattern = expression` condition in `if` or `while`.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `val Some(x) = value`:
    ///
    /// ```text
    /// BindingCondition
    /// +-- ValKw "val" [token]
    /// +-- Pattern (Some(x)) [node] -> pattern()
    /// +-- Eq "=" [token]
    /// `-- PathExpr (value) [node] [Expr #0] -> initializer()
    /// ```
    BindingCondition,
    BindingCondition
);

ast_node!(
    /// A syntactic pattern; binding and constructor meanings are resolved later.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Some(x)`:
    ///
    /// ```text
    /// Pattern
    /// +-- PathExpr (Some) [node] [Expr #0] -> range_bounds(), path()
    /// +-- LParen "(" [token]
    /// +-- Pattern (x) [node] -> elements()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `_`:
    ///
    /// ```text
    /// Pattern
    /// `-- PathExpr (_) [node] [Expr #0] -> range_bounds(), path()
    /// ```
    ///
    /// `1`:
    ///
    /// ```text
    /// Pattern
    /// `-- Literal (1) [node] [Expr #0] -> range_bounds(), literal()
    /// ```
    ///
    /// `1..=3`:
    ///
    /// ```text
    /// Pattern
    /// +-- Literal (1) [node] [Expr #0] -> range_bounds(), literal()
    /// +-- DotDotEq "..=" [token]
    /// `-- Literal (3) [node] [Expr #1] -> range_bounds()
    /// ```
    ///
    /// `A | B`:
    ///
    /// ```text
    /// Pattern
    /// +-- Pattern (A) [node] -> elements()
    /// +-- Pipe "|" [token]
    /// `-- Pattern (B) [node] -> elements()
    /// ```
    ///
    /// `(a, b)`:
    ///
    /// ```text
    /// Pattern
    /// +-- LParen "(" [token]
    /// +-- Pattern (a) [node] -> elements()
    /// +-- Comma "," [token]
    /// +-- Pattern (b) [node] -> elements()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `(a)`:
    ///
    /// ```text
    /// Pattern
    /// +-- LParen "(" [token]
    /// +-- Pattern (a) [node] -> elements()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `Point { x, y: n }`:
    ///
    /// ```text
    /// Pattern
    /// +-- PathExpr (Point) [node] [Expr #0] -> range_bounds(), path()
    /// +-- LBrace "{" [token]
    /// +-- PatternField (x) [node] -> fields()
    /// +-- Comma "," [token]
    /// +-- PatternField (y: n) [node] -> fields()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// The diagrams show constructor, wildcard, literal, range, alternative, tuple,
    /// grouped and struct forms. `elements()` reads direct nested patterns; `fields()`
    /// reads direct `PatternField` nodes. `range_bounds()` reads direct literals/paths,
    /// including those on non-range patterns: check `range_inclusive()` first.
    /// `(a)` is grouped; `(a,)` is a tuple. These views do not resolve identifiers.
    Pattern,
    Pattern
);

ast_node!(
    /// A named struct-pattern field with an optional explicit subpattern.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `x: n`:
    ///
    /// ```text
    /// PatternField
    /// +-- Name (x) [node] -> name()
    /// +-- Colon ":" [token]
    /// `-- Pattern (n) [node] -> pattern()
    /// ```
    ///
    /// Shorthand `Point { x }` has only `Name`; `pattern()` returns `None`.
    /// An explicit `x: n` adds a direct subpattern.
    PatternField,
    PatternField
);

ast_node!(
    /// A comma-separated tuple expression, including the empty tuple.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `(1, 2)`:
    ///
    /// ```text
    /// TupleExpr
    /// +-- LParen "(" [token]
    /// +-- Literal (1) [node] [Expr #0] -> elements()
    /// +-- Comma "," [token]
    /// +-- Literal (2) [node] [Expr #1] -> elements()
    /// `-- RParen ")" [token]
    /// ```
    ///
    /// `()` has no expression children; `(x,)` is a one-element tuple.
    /// `(x)` is instead a [`ParenExpr`].
    TupleExpr,
    TupleExpr
);

ast_node!(
    /// An array list or repeated-element array expression.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `[1, 2]`:
    ///
    /// ```text
    /// ArrayExpr
    /// +-- LBracket "[" [token]
    /// +-- Literal (1) [node] [Expr #0] -> elements()
    /// +-- Comma "," [token]
    /// +-- Literal (2) [node] [Expr #1] -> elements()
    /// `-- RBracket "]" [token]
    /// ```
    ///
    /// `[0; 3]`:
    ///
    /// ```text
    /// ArrayExpr
    /// +-- LBracket "[" [token]
    /// +-- Literal (0) [node] [Expr #0] -> elements()
    /// +-- Semi ";" [token]
    /// +-- Literal (3) [node] [Expr #1] -> elements()
    /// `-- RBracket "]" [token]
    /// ```
    ///
    /// For `[value; count]`, `elements()` yields both the value and the count expression
    /// in source order; it does not expand the repetition. `is_repeat()` detects `;`.
    ArrayExpr,
    ArrayExpr
);

ast_node!(
    /// An `f"..."` string containing text tokens and expression holes.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `f"value={x}"`:
    ///
    /// ```text
    /// InterpolatedString
    /// +-- FormatStart "f\"" [token]
    /// +-- FormatText "value=" [token]
    /// +-- Interpolation ({x}) [node]
    /// `-- FormatEnd "\"" [token]
    /// ```
    ///
    /// Text is retained in `FormatText` tokens; expression holes are `Interpolation`
    /// nodes. Inspect the underlying syntax elements to preserve their interleaving.
    /// Escaped or doubled braces inside text are not expression-hole delimiters.
    InterpolatedString,
    InterpolatedString
);

ast_node!(
    /// One expression hole, optionally requesting debug formatting with `:?`.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `{x:?}`:
    ///
    /// ```text
    /// Interpolation
    /// +-- FormatOpen "{" [token]
    /// +-- PathExpr (x) [node] [Expr #0] -> expr()
    /// +-- Colon ":" [token]
    /// +-- Question "?" [token]
    /// `-- FormatClose "}" [token]
    /// ```
    ///
    /// `debug()` checks for the direct `:` marker. Valid debug syntax is `:?`; the
    /// accessor does not independently validate the following `?` token.
    Interpolation,
    Interpolation
);

/// A typed choice of expression views; no additional CST node is allocated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// A block with statements and an optional tail, such as `{ val x = 1; x + 2 }`.
    BlockExpr(BlockExpr),
    /// An unresolved name or path, such as `value`, `math::total`, or `Box<i32>::make`.
    PathExpr(PathExpr),
    /// A literal spelling, such as `42`, `true`, or `"text"`.
    Literal(Literal),
    /// Explicit grouping, such as `(x + 1)`; `(x,)` is a tuple instead.
    ParenExpr(ParenExpr),
    /// A unary operation, such as `-count` or `!ready`.
    PrefixExpr(PrefixExpr),
    /// A cast with a written target type, such as `value as i64`.
    CastExpr(CastExpr),
    /// Result/Option propagation, such as `parse(text)?`.
    PropagateExpr(PropagateExpr),
    /// An await operation, such as `fetch().await`.
    AwaitExpr(AwaitExpr),
    /// A binary operation, such as `a + b`, `x == y`, or `ready && enabled`.
    BinaryExpr(BinaryExpr),
    /// A range, such as `1..3`, `1..=3`, `..end`, or `start..`.
    RangeExpr(RangeExpr),
    /// A call, such as `add(a, b)`, `identity<i32>(x)`, or `obj.read()`.
    CallExpr(CallExpr),
    /// Member access, such as `player.hp` or the callee `obj.read` in `obj.read()`.
    FieldExpr(FieldExpr),
    /// Indexed access, such as `values[i + 1]`.
    IndexExpr(IndexExpr),
    /// A conditional, such as `if ready { a } else { b }` or `if val Some(x) = value { x }`.
    IfExpr(IfExpr),
    /// Struct construction, such as `Point { x: 1, y, ..base }`, including shorthand and update.
    StructExpr(StructExpr),
    /// Pattern selection, such as `match value { Some(x) => x, _ => 0 }`.
    MatchExpr(MatchExpr),
    /// A loop in expression position, such as the initializer in `val x = loop { break 1; };`.
    LoopExpr(LoopExpr),
    /// An anonymous callable, such as `|x| x + 1`, `|| 1`, or `async || fetch().await`.
    ClosureExpr(ClosureExpr),
    /// A cold future, such as `async { fetch().await }`.
    AsyncBlockExpr(AsyncBlockExpr),
    /// A tuple, such as `(x, y)`, `(x,)`, or `()`; `(x)` is grouping instead.
    TupleExpr(TupleExpr),
    /// An array list `[a, b]` or repeated array `[value; count]`.
    ArrayExpr(ArrayExpr),
    /// A formatted string, such as `f"value={x}"` or `f"{x:?}"`.
    InterpolatedString(InterpolatedString),
}

impl AstNode for Expr {
    fn can_cast(kind: SyntaxKind) -> bool {
        matches!(
            kind,
            SyntaxKind::BlockExpr
                | SyntaxKind::PathExpr
                | SyntaxKind::Literal
                | SyntaxKind::ParenExpr
                | SyntaxKind::AwaitExpr
                | SyntaxKind::PropagateExpr
                | SyntaxKind::CastExpr
                | SyntaxKind::PrefixExpr
                | SyntaxKind::BinaryExpr
                | SyntaxKind::RangeExpr
                | SyntaxKind::CallExpr
                | SyntaxKind::FieldExpr
                | SyntaxKind::IndexExpr
                | SyntaxKind::IfExpr
                | SyntaxKind::StructExpr
                | SyntaxKind::MatchExpr
                | SyntaxKind::LoopExpr
                | SyntaxKind::ClosureExpr
                | SyntaxKind::AsyncBlockExpr
                | SyntaxKind::TupleExpr
                | SyntaxKind::InterpolatedString
                | SyntaxKind::ArrayExpr
        )
    }

    fn cast(syntax: SyntaxNode) -> Option<Self> {
        match syntax.kind() {
            SyntaxKind::BlockExpr => BlockExpr::cast(syntax).map(Self::BlockExpr),
            SyntaxKind::PathExpr => PathExpr::cast(syntax).map(Self::PathExpr),
            SyntaxKind::Literal => Literal::cast(syntax).map(Self::Literal),
            SyntaxKind::ParenExpr => ParenExpr::cast(syntax).map(Self::ParenExpr),
            SyntaxKind::AwaitExpr => AwaitExpr::cast(syntax).map(Self::AwaitExpr),
            SyntaxKind::PropagateExpr => PropagateExpr::cast(syntax).map(Self::PropagateExpr),
            SyntaxKind::CastExpr => CastExpr::cast(syntax).map(Self::CastExpr),
            SyntaxKind::PrefixExpr => PrefixExpr::cast(syntax).map(Self::PrefixExpr),
            SyntaxKind::BinaryExpr => BinaryExpr::cast(syntax).map(Self::BinaryExpr),
            SyntaxKind::RangeExpr => RangeExpr::cast(syntax).map(Self::RangeExpr),
            SyntaxKind::CallExpr => CallExpr::cast(syntax).map(Self::CallExpr),
            SyntaxKind::FieldExpr => FieldExpr::cast(syntax).map(Self::FieldExpr),
            SyntaxKind::IndexExpr => IndexExpr::cast(syntax).map(Self::IndexExpr),
            SyntaxKind::IfExpr => IfExpr::cast(syntax).map(Self::IfExpr),
            SyntaxKind::StructExpr => StructExpr::cast(syntax).map(Self::StructExpr),
            SyntaxKind::MatchExpr => MatchExpr::cast(syntax).map(Self::MatchExpr),
            SyntaxKind::LoopExpr => LoopExpr::cast(syntax).map(Self::LoopExpr),
            SyntaxKind::ClosureExpr => ClosureExpr::cast(syntax).map(Self::ClosureExpr),
            SyntaxKind::AsyncBlockExpr => AsyncBlockExpr::cast(syntax).map(Self::AsyncBlockExpr),
            SyntaxKind::TupleExpr => TupleExpr::cast(syntax).map(Self::TupleExpr),
            SyntaxKind::InterpolatedString => {
                InterpolatedString::cast(syntax).map(Self::InterpolatedString)
            }
            SyntaxKind::ArrayExpr => ArrayExpr::cast(syntax).map(Self::ArrayExpr),
            _ => None,
        }
    }

    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::BlockExpr(node) => node.syntax(),
            Self::PathExpr(node) => node.syntax(),
            Self::Literal(node) => node.syntax(),
            Self::ParenExpr(node) => node.syntax(),
            Self::PropagateExpr(node) => node.syntax(),
            Self::AwaitExpr(node) => node.syntax(),
            Self::CastExpr(node) => node.syntax(),
            Self::PrefixExpr(node) => node.syntax(),
            Self::BinaryExpr(node) => node.syntax(),
            Self::RangeExpr(node) => node.syntax(),
            Self::CallExpr(node) => node.syntax(),
            Self::FieldExpr(node) => node.syntax(),
            Self::IndexExpr(node) => node.syntax(),
            Self::IfExpr(node) => node.syntax(),
            Self::StructExpr(node) => node.syntax(),
            Self::MatchExpr(node) => node.syntax(),
            Self::LoopExpr(node) => node.syntax(),
            Self::ClosureExpr(node) => node.syntax(),
            Self::AsyncBlockExpr(node) => node.syntax(),
            Self::TupleExpr(node) => node.syntax(),
            Self::InterpolatedString(node) => node.syntax(),
            Self::ArrayExpr(node) => node.syntax(),
        }
    }
}

impl BlockExpr {
    /// Iterates `val x = 1;` then `log(x);` in `{ val x = 1; log(x); x }`.
    /// The tail `x` is excluded; `{ x }` yields no statements.
    pub fn statements(&self) -> impl Iterator<Item = Stmt> {
        support::children(self.syntax())
    }

    /// Returns `x + 2` in `{ val x = 1; x + 2 }`.
    /// Returns `None` for `{ x + 2; }`: the semicolon makes it a statement.
    /// Only the last direct expression is selected, never one nested in a statement.
    pub fn tail_expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl PathExpr {
    /// Returns the type wrapper for `<T as Read>::make`.
    /// Returns `None` for ordinary paths such as `math::total`.
    pub fn qualified_type(&self) -> Option<TypeRef> {
        self.syntax().children().find_map(TypeRef::cast)
    }

    /// Returns `<i32>` in `Box<i32>::make` or the function value `identity::<i32>`.
    /// Returns `None` for `math::total`; qualified forms use `qualified_type()`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the member `make` in `Box<i32>::make`.
    /// For `math::total`, the names are inside `path()`, so this returns `None`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `math::total` for `math :: total`, omitting trivia.
    /// `Box<i32>::make` yields `Box::make`; `<T as Read>::make` yields only `make`.
    /// The function value `identity::<i32>` yields `identity`, without a member name.
    /// Returns `None` if the required path/name components are missing.
    pub fn name_text(&self) -> Option<String> {
        if let Some(ty) = self.qualified_type() {
            return ty.qualified_type()?.member()?.text();
        }
        if self.generic_args().is_some() && self.name().is_some() {
            return Some(format!(
                "{}::{}",
                self.path()?.text()?,
                self.name()?.text()?
            ));
        }
        self.path()
            .and_then(|path| path.text())
            .or_else(|| self.name().and_then(|name| name.text()))
    }

    /// Returns `math::total` in `math::total`, or `Box` in `Box<i32>::make`.
    /// Returns `None` for `<T as Read>::make`; use `qualified_type()` for that form.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }
}

impl Literal {
    /// Returns the source spelling: `42` stays `42`, and `"a\n"` keeps its quotes and escape.
    /// Copies the first token without decoding it; an empty recovered literal yields `None`.
    pub fn text(&self) -> Option<String> {
        self.syntax()
            .first_token()
            .map(|token| token.text().to_string())
    }

    /// Returns `SyntaxKind::Number` for `42` and `SyntaxKind::TrueKw` for `true`.
    /// Reads the first token; an empty recovered literal yields `None`.
    pub fn kind(&self) -> Option<SyntaxKind> {
        self.syntax().first_token().map(|token| token.kind())
    }
}

impl ParenExpr {
    /// Returns `x + 1` in `(x + 1)`, without the grouping parentheses.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl AwaitExpr {
    /// Returns `fetch()` in `fetch().await`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl PropagateExpr {
    /// Returns `parse(text)` in `parse(text)?`, without the propagation operator.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl PrefixExpr {
    /// Returns `count` in `-count`, or `ready` in `!ready`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns `Some(SyntaxKind::Minus)` for `-count` and `Some(SyntaxKind::Bang)` for `!ready`.
    /// Returns `None` if neither direct operator token is present.
    pub fn operator(&self) -> Option<SyntaxKind> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Token(token)
                    if matches!(token.kind(), SyntaxKind::Minus | SyntaxKind::Bang) =>
                {
                    Some(token.kind())
                }
                _ => None,
            })
    }
}

impl BinaryExpr {
    /// Returns `a` in `a + b * c`.
    /// Selects the first direct expression; it does not descend into either operand.
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the whole `b * c` expression in `a + b * c`.
    /// Selects the second direct expression, or `None` if recovery leaves fewer than two.
    pub fn rhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }

    /// Returns `Some(SyntaxKind::Plus)` for the outer expression in `a + b * c`.
    /// Ignores nested operators such as `*`; returns `None` if no direct binary operator remains.
    pub fn operator(&self) -> Option<SyntaxKind> {
        self.syntax()
            .children_with_tokens()
            .find_map(|element| match element {
                NodeOrToken::Token(token)
                    if matches!(
                        token.kind(),
                        SyntaxKind::Plus
                            | SyntaxKind::Minus
                            | SyntaxKind::Star
                            | SyntaxKind::Slash
                            | SyntaxKind::Percent
                            | SyntaxKind::Amp
                            | SyntaxKind::Pipe
                            | SyntaxKind::Caret
                            | SyntaxKind::Shl
                            | SyntaxKind::Shr
                            | SyntaxKind::IdentityEq
                            | SyntaxKind::IdentityNotEq
                            | SyntaxKind::EqEq
                            | SyntaxKind::NotEq
                            | SyntaxKind::Lt
                            | SyntaxKind::Gt
                            | SyntaxKind::Le
                            | SyntaxKind::Ge
                            | SyntaxKind::AmpAmp
                            | SyntaxKind::PipePipe
                    ) =>
                {
                    Some(token.kind())
                }
                _ => None,
            })
    }
}

impl RangeExpr {
    /// Returns `start` in `start..end` and `start..=end`.
    /// Returns `None` for `..end` or `..`; selects only expressions before the range token.
    pub fn start(&self) -> Option<Expr> {
        self.syntax()
            .children_with_tokens()
            .take_while(|part| !matches!(part.kind(), SyntaxKind::DotDot | SyntaxKind::DotDotEq))
            .filter_map(|part| part.into_node().and_then(Expr::cast))
            .next()
    }

    /// Returns `end` in `start..end` and `..=end`.
    /// Returns `None` for `start..`, `..`, or a missing recovered end.
    pub fn end(&self) -> Option<Expr> {
        self.syntax()
            .children_with_tokens()
            .skip_while(|part| !matches!(part.kind(), SyntaxKind::DotDot | SyntaxKind::DotDotEq))
            .filter_map(|part| part.into_node().and_then(Expr::cast))
            .next()
    }

    /// Whether the range includes its end: true for `1..=3`, false for `1..3`.
    /// Checks the direct `..=` token without validating either endpoint.
    pub fn inclusive(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::DotDotEq).is_some()
    }
}

impl CallExpr {
    /// Returns `<i32>` in `identity<i32>(value)`; returns `None` in `identity(value)`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns `add` in `add(a, b)`, or `obj.read` in `obj.read()`.
    /// Selects the first direct expression, excluding type arguments.
    pub fn callee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Iterates `a` then `b + 1` in `add<i32>(a, b + 1)`; `add()` yields no arguments.
    /// Skips the first direct expression (the callee); tokens and type arguments do not count.
    pub fn args(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast).skip(1)
    }
}

impl FieldExpr {
    /// Returns `player` in `player.hp`, or `players[i]` in `players[i].hp`.
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the field name `hp` in `player.hp`.
    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    /// Returns `Some("hp")` for `player.hp`, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }
}

impl IndexExpr {
    /// Returns `values` in `values[i + 1]`, without inspecting the index.
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns `i + 1` in `values[i + 1]`.
    /// Selects the second direct expression, or `None` if recovery leaves fewer than two.
    pub fn index(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl IfExpr {
    /// Returns `val Some(x) = value` in `if val Some(x) = value { x }`.
    /// Returns `None` for a Boolean condition such as `if ready { work(); }`.
    pub fn binding_condition(&self) -> Option<BindingCondition> {
        support::child(self.syntax())
    }

    /// Returns `ready` in `if ready { work(); }`.
    /// For `if val Some(x) = value { x }`, returns the then block `{ x }` instead:
    /// the initializer is nested under `binding_condition()`. Check that accessor first.
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns `{ a }` in `if ready { a } else { b }`.
    /// Selects the first direct block, including for an `if val` condition.
    pub fn then_branch(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }

    /// Returns `{ b }` in `if ready { a } else { b }`, or the nested `if` in `else if`.
    /// Returns `None` when there is no expression after the first direct block.
    /// Recovery can change these positional roles; see the type diagrams.
    pub fn else_branch(&self) -> Option<Expr> {
        let mut blocks = self.syntax().children().filter_map(BlockExpr::cast);
        let then_branch = blocks.next()?;

        let mut seen_then_branch = false;
        self.syntax()
            .children()
            .filter_map(Expr::cast)
            .find(|expr| {
                if !seen_then_branch && expr.syntax() == then_branch.syntax() {
                    seen_then_branch = true;
                    return false;
                }

                seen_then_branch
            })
    }
}

impl BindingCondition {
    /// Returns `Some(x)` in the condition `val Some(x) = value`.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }

    /// Returns `value` in the condition `val Some(x) = value`.
    pub fn initializer(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl StructExpr {
    /// Returns `<i32>` in `Box<i32> { value: 1 }`; returns `None` in `Point { x: 1 }`.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns `Point` in `Point { x: 1 }`, or `Box` in `Box<i32> { value: 1 }`.
    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    /// Returns the explicit field list, including shorthand fields but never the base.
    /// In `Point { x: 1, y, ..base }` this selects `x: 1, y,`; in `Point { ..base }`
    /// it selects an empty list. The braces belong to the struct expression.
    pub fn field_list(&self) -> Option<FieldInitList> {
        self.syntax()
            .children()
            .filter_map(FieldInitList::cast)
            .next()
    }

    /// Returns the written `..base` node, independently of whether its operand exists.
    /// Ordinary construction returns `None`; recovered `Point { .. }` returns `Some`
    /// with `value() == None`. Only the first direct base is selected; repeated bases
    /// retained under recovery error nodes are not additional valid bases.
    pub fn update_base(&self) -> Option<StructUpdateBase> {
        support::child(self.syntax())
    }
}

impl StructUpdateBase {
    /// Returns `base` in `Point { ..base }`, or `None` for recovered `Point { .. }`.
    /// The surrounding base node records marker presence independently of this operand.
    pub fn value(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl FieldInitList {
    /// Iterates explicit `x: 1` then shorthand `y` in `{ x: 1, y, ..base }`.
    /// Both `{}` and `{ ..base }` yield no fields; the update base is never a field.
    pub fn fields(&self) -> impl Iterator<Item = FieldInit> {
        self.syntax().children().filter_map(FieldInit::cast)
    }
}

impl FieldInit {
    /// Returns `x` in both `x: value` and shorthand `x`.
    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    /// Returns `Some("x")` for both `x: value` and shorthand `x`.
    /// Returns `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns `value` in `x: value`; returns `None` for shorthand `x`.
    /// The shorthand's implicit value is not an expression child.
    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl MatchExpr {
    /// Returns `value` in `match value { Some(x) => x, _ => 0 }`.
    pub fn scrutinee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the arm list `Some(x) => x, _ => 0` in `match value { Some(x) => x, _ => 0 }`.
    /// The braces belong to the match expression; use the list's `arms()` to visit each arm.
    pub fn arms(&self) -> Option<MatchArmList> {
        self.syntax()
            .children()
            .filter_map(MatchArmList::cast)
            .next()
    }
}

impl LoopExpr {
    /// Returns `{ break 42; }` in the expression `loop { break 42; }`.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl AsyncBlockExpr {
    /// Returns `{ value }` in `async { value }`, or `None` for a missing body.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ClosureExpr {
    /// Whether the closure explicitly starts with `async`: true for `async || 1`, false for `|| 1`.
    pub fn is_async(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::AsyncKw)
    }

    /// Iterates `x: i32` then `y` in `|x: i32, y| x + y`; `|| 1` yields no parameters.
    /// Reads parameters inside direct parameter-list nodes, excluding the body.
    pub fn params(&self) -> impl Iterator<Item = ClosureParam> {
        self.syntax()
            .children()
            .filter_map(ClosureParamList::cast)
            .flat_map(|list| {
                list.syntax()
                    .children()
                    .filter_map(ClosureParam::cast)
                    .collect::<Vec<_>>()
            })
    }

    /// Returns `x + 1` in `|x| x + 1`, or `{ x + 1 }` in `|x| { x + 1 }`.
    /// Selects the last direct expression.
    pub fn body(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl ClosureParam {
    /// Returns `x` in both `|x| x` and `|x: i32| x`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `i32` for `x` in `|x: i32| x`; returns `None` in `|x| x`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl MatchArmList {
    /// Iterates `Some(x) => x` then `_ => 0` in `{ Some(x) => x, _ => 0 }`.
    pub fn arms(&self) -> impl Iterator<Item = MatchArm> {
        self.syntax().children().filter_map(MatchArm::cast)
    }
}

impl MatchArm {
    /// Returns `Some(x)` in `Some(x) if x > 0 => x`.
    pub fn pattern(&self) -> Option<Pattern> {
        self.syntax().children().filter_map(Pattern::cast).next()
    }

    /// Returns the result `x` in `Some(x) if x > 0 => x`, excluding the guard `x > 0`.
    /// Selects the last direct expression.
    pub fn expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }

    /// Returns `x > 0` in `Some(x) if x > 0 => x`; returns `None` in `Some(x) => x`.
    /// With an `if` token, selects the first direct expression; recovery can leave it sharing the result node.
    pub fn guard(&self) -> Option<Expr> {
        if support::token(self.syntax(), SyntaxKind::IfKw).is_some() {
            self.syntax().children().filter_map(Expr::cast).next()
        } else {
            None
        }
    }
}

impl Pattern {
    /// Whether the pattern has alternatives, such as `Some(_) | None`.
    /// Checks a direct `|`; the outer `(Some(_) | None)` is grouped, so it returns false.
    pub fn is_or(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::Pipe).is_some()
    }

    /// Returns `Some(true)` for `1..=3`, `Some(false)` for `1..3`, and `None` for `1`.
    /// Checks only the direct range token, not whether both bounds are valid.
    /// Struct-pattern rest, such as `Point { .. }`, is not a range.
    pub fn range_inclusive(&self) -> Option<bool> {
        if self.is_struct() {
            return None;
        }
        if support::token(self.syntax(), SyntaxKind::DotDotEq).is_some() {
            Some(true)
        } else if support::token(self.syntax(), SyntaxKind::DotDot).is_some() {
            Some(false)
        } else {
            None
        }
    }

    /// Iterates `1` then `MAX` in `1..=MAX`, as literal and path bounds respectively.
    /// Also yields `1` for the non-range pattern `1`; check `range_inclusive()` first.
    pub fn range_bounds(&self) -> impl Iterator<Item = PatternBound> {
        self.syntax().children().filter_map(|child| {
            Literal::cast(child.clone())
                .map(PatternBound::Literal)
                .or_else(|| PathExpr::cast(child).map(PatternBound::Path))
        })
    }

    /// Iterates `x` then `y` in `(x, y)` or `Pair(x, y)`, and `A` then `B` in `A | B`.
    /// `(x)` yields one nested pattern; a plain `x` yields none. Does not descend into fields.
    pub fn elements(&self) -> impl Iterator<Item = Pattern> {
        self.syntax().children().filter_map(Pattern::cast)
    }

    /// Whether the pattern's first direct path spells `_`.
    /// True for `_`, false for `value` or the outer grouped pattern `(_)`.
    pub fn is_wildcard(&self) -> bool {
        self.path().and_then(|path| path.name_text()).as_deref() == Some("_")
    }

    /// Returns `Some` in `Some(x)`, `Point` in `Point { x }`, or `x` in plain `x`.
    /// Returns `None` for literal `1` and tuple `(x, y)` patterns.
    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    /// Returns the literal in `42`, or the first literal `1` in `1..=3`.
    /// Returns `None` for `name`; use `range_bounds()` to read both range endpoints.
    pub fn literal(&self) -> Option<Literal> {
        self.syntax().children().filter_map(Literal::cast).next()
    }

    /// Whether this is a struct-shaped pattern, such as `Point { x, y }`.
    /// Recognized by a direct `{` token.
    pub fn is_struct(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::LBrace).is_some()
    }

    /// Whether this struct pattern contains an explicit rest marker, as in `Point { x, .. }`.
    /// Reads only a direct `..` token; nested rests and ordinary ranges return false.
    /// The marker is not a field or a nested pattern, and may omit zero fields.
    pub fn has_rest(&self) -> bool {
        self.is_struct() && support::token(self.syntax(), SyntaxKind::DotDot).is_some()
    }

    /// Whether this is a tuple-struct-shaped pattern, such as `Some(value)`.
    /// Requires a direct path and `(` token; false for `(value,)`.
    pub fn is_tuple_struct(&self) -> bool {
        self.path().is_some() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    /// Whether this is a tuple or grouped pattern: `()`, `(x,)`, `(x, y)`, or `(x)`.
    /// False for `Some(x)`; use `is_grouped()` to distinguish `(x)` from tuple patterns.
    pub fn is_tuple(&self) -> bool {
        self.path().is_none() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    /// Whether parentheses group a single pattern, such as `(x)`.
    /// False for tuple patterns `()`, `(x,)`, `(x, y)` and constructor patterns `Some(x)`.
    pub fn is_grouped(&self) -> bool {
        self.is_tuple()
            && support::token(self.syntax(), SyntaxKind::Comma).is_none()
            && self.elements().count() == 1
    }

    /// Iterates shorthand `x` then `y: value` in `Point { x, y: value }`.
    /// Tuple patterns such as `(x, y)` yield no fields; use `elements()` for them.
    pub fn fields(&self) -> impl Iterator<Item = PatternField> {
        self.syntax().children().filter_map(PatternField::cast)
    }
}

/// A literal or path selected as a possible pattern range endpoint.
pub enum PatternBound {
    /// A literal endpoint, such as `1` in `1..=MAX`.
    Literal(Literal),
    /// A named endpoint, such as `MAX` in `1..=MAX`; constant resolution happens later.
    Path(PathExpr),
}

impl PatternField {
    /// Returns the field name `x` in both `Point { x }` and `Point { x: value }`.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns `value` for `x: value` in `Point { x: value }`.
    /// Returns `None` for shorthand `Point { x }`; no implicit nested pattern is created.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }
}

impl TupleExpr {
    /// Iterates `x` then `y + 1` in `(x, y + 1)`; `(x,)` yields one element and `()` none.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}

impl ArrayExpr {
    /// Whether this is a repeated array: true for `[0; 3]`, false for `[0, 0, 0]`.
    /// Checks the direct `;` token without evaluating the count.
    pub fn is_repeat(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|part| part.kind() == SyntaxKind::Semi)
    }

    /// Iterates `a` then `b` in `[a, b]`; `[]` yields no elements.
    /// For `[value; count]`, yields `value` then `count`, not expanded copies of `value`.
    /// Check `is_repeat()` before interpreting the children.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}

impl Interpolation {
    /// Returns `value` in both `f"{value}"` and `f"{value:?}"`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Whether interpolation requests debug formatting: true in `f"{value:?}"`, false in `f"{value}"`.
    /// Checks only the direct `:` marker; does not revalidate a recovered `:?` suffix.
    pub fn debug(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::Colon)
    }
}

impl CastExpr {
    /// Returns `value` in `value as i64`.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns the target type `i64` in `value as i64`.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}
