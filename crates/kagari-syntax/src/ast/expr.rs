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
    /// A constructor path followed by named field initializers.
    ///
    /// See [AST conventions](crate::ast) for storage and diagram notation.
    ///
    /// # Tree shape
    ///
    /// Trivia is omitted; child-node descendants are collapsed.
    ///
    /// `Point { x: 1, y: 2 }`:
    ///
    /// ```text
    /// StructExpr
    /// +-- PathExpr (Point) [node] [Expr #0] -> path()
    /// +-- LBrace "{" [token]
    /// +-- FieldInitList (x: 1, y: 2) [node] -> field_list()
    /// `-- RBrace "}" [token]
    /// ```
    ///
    /// Braces are direct tokens here; `FieldInitList` owns the comma-separated fields.
    /// Generic constructor arguments, when present, are direct children of this node.
    StructExpr,
    StructExpr
);

ast_node!(
    /// The field initializer list of a struct expression.
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
    /// Iterates direct `FieldInit` children in source order; commas are tokens.
    /// The enclosing braces belong to `StructExpr`.
    FieldInitList,
    FieldInitList
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
    /// The [`BlockExpr`] view; see its node diagram.
    BlockExpr(BlockExpr),
    /// The [`PathExpr`] view; see its node diagram.
    PathExpr(PathExpr),
    /// The [`Literal`] view; see its node diagram.
    Literal(Literal),
    /// The [`ParenExpr`] view; see its node diagram.
    ParenExpr(ParenExpr),
    /// The [`PrefixExpr`] view; see its node diagram.
    PrefixExpr(PrefixExpr),
    /// The [`CastExpr`] view; see its node diagram.
    CastExpr(CastExpr),
    /// The [`PropagateExpr`] view; see its node diagram.
    PropagateExpr(PropagateExpr),
    /// The [`AwaitExpr`] view; see its node shape.
    AwaitExpr(AwaitExpr),
    /// The [`BinaryExpr`] view; see its node diagram.
    BinaryExpr(BinaryExpr),
    /// The [`RangeExpr`] view; see its node diagram.
    RangeExpr(RangeExpr),
    /// The [`CallExpr`] view; see its node diagram.
    CallExpr(CallExpr),
    /// The [`FieldExpr`] view; see its node diagram.
    FieldExpr(FieldExpr),
    /// The [`IndexExpr`] view; see its node diagram.
    IndexExpr(IndexExpr),
    /// The [`IfExpr`] view; see its node diagram.
    IfExpr(IfExpr),
    /// The [`StructExpr`] view; see its node diagram.
    StructExpr(StructExpr),
    /// The [`MatchExpr`] view; see its node diagram.
    MatchExpr(MatchExpr),
    /// The [`LoopExpr`] view; see its node diagram.
    LoopExpr(LoopExpr),
    /// The [`ClosureExpr`] view; see its node diagram.
    ClosureExpr(ClosureExpr),
    /// The [`TupleExpr`] view; see its node diagram.
    TupleExpr(TupleExpr),
    /// The [`ArrayExpr`] view; see its node diagram.
    ArrayExpr(ArrayExpr),
    /// The [`InterpolatedString`] view; see its node diagram.
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
            Self::TupleExpr(node) => node.syntax(),
            Self::InterpolatedString(node) => node.syntax(),
            Self::ArrayExpr(node) => node.syntax(),
        }
    }
}

impl BlockExpr {
    /// Iterates direct `Stmt` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn statements(&self) -> impl Iterator<Item = Stmt> {
        support::children(self.syntax())
    }

    /// Returns the last direct expression, or `None` when no tail is present.
    /// Statement wrappers and expressions nested inside them are excluded.
    pub fn tail_expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl PathExpr {
    /// Returns the qualified form: the first direct `TypeRef` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn qualified_type(&self) -> Option<TypeRef> {
        self.syntax().children().find_map(TypeRef::cast)
    }

    /// Returns the generic arguments: the first direct `GenericArgList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns a normalized spelling, omitting generic arguments.
    /// For a qualified form, returns only its member name; returns `None` if the
    /// required path/name components are missing. See the type diagrams.
    pub fn name_text(&self) -> Option<String> {
        if let Some(ty) = self.qualified_type() {
            return ty.qualified_type()?.member()?.text();
        }
        if self.generic_args().is_some() {
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

    /// Returns the path: the first direct `Path` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<Path> {
        support::child(self.syntax())
    }
}

impl Literal {
    /// Copies the first token's spelling, including quotes/escapes; `None` for an empty node.
    pub fn text(&self) -> Option<String> {
        self.syntax()
            .first_token()
            .map(|token| token.text().to_string())
    }

    /// Returns the first token's kind, or `None` for an empty recovered literal.
    pub fn kind(&self) -> Option<SyntaxKind> {
        self.syntax().first_token().map(|token| token.kind())
    }
}

impl ParenExpr {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl AwaitExpr {
    /// Returns the operand, or `None` during recovery.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl PropagateExpr {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl PrefixExpr {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns the first recognized direct operator token's kind.
    /// Returns `None` if no operator from this expression/statement family is present.
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
    /// Returns the left operand: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn lhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the right operand: the second direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn rhs(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }

    /// Returns the first recognized direct operator token's kind.
    /// Returns `None` if no operator from this expression/statement family is present.
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
    /// Returns the first direct expression before the range operator.
    /// An omitted start, as in `..end`, yields `None`.
    pub fn start(&self) -> Option<Expr> {
        self.syntax()
            .children_with_tokens()
            .take_while(|part| !matches!(part.kind(), SyntaxKind::DotDot | SyntaxKind::DotDotEq))
            .filter_map(|part| part.into_node().and_then(Expr::cast))
            .next()
    }

    /// Returns the first direct expression after the range operator.
    /// An open exclusive end or a missing recovered end yields `None`.
    pub fn end(&self) -> Option<Expr> {
        self.syntax()
            .children_with_tokens()
            .skip_while(|part| !matches!(part.kind(), SyntaxKind::DotDot | SyntaxKind::DotDotEq))
            .filter_map(|part| part.into_node().and_then(Expr::cast))
            .next()
    }

    /// Whether a direct `..=` token is present; false for `..`.
    pub fn inclusive(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::DotDotEq).is_some()
    }
}

impl CallExpr {
    /// Returns the generic arguments: the first direct `GenericArgList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the callee: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn callee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Iterates direct expressions after the first (callee), using `skip(1)`.
    /// Tokens and generic arguments are excluded before positions are counted.
    /// Returns an empty iterator when no matching children remain.
    pub fn args(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast).skip(1)
    }
}

impl FieldExpr {
    /// Returns the receiver: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }
}

impl IndexExpr {
    /// Returns the receiver: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn receiver(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the index expression: the second direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn index(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).nth(1)
    }
}

impl IfExpr {
    /// Returns the binding condition: the first direct `BindingCondition` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn binding_condition(&self) -> Option<BindingCondition> {
        support::child(self.syntax())
    }

    /// Returns the first direct expression (`next()` after filtering).
    /// Check `binding_condition()` first: its initializer is nested, so this
    /// accessor selects the then block for binding-condition syntax.
    pub fn condition(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the then block: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn then_branch(&self) -> Option<BlockExpr> {
        self.syntax().children().filter_map(BlockExpr::cast).next()
    }

    /// Returns the first expression after the first direct block.
    /// Valid syntax yields an else block or nested `if`; no such child yields `None`.
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
    /// Returns the pattern: the first direct `Pattern` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }

    /// Returns the initializer: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn initializer(&self) -> Option<Expr> {
        support::child(self.syntax())
    }
}

impl StructExpr {
    /// Returns the generic arguments: the first direct `GenericArgList` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn generic_args(&self) -> Option<GenericArgList> {
        support::child(self.syntax())
    }

    /// Returns the path: the first direct `PathExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    /// Returns the field list: the first direct `FieldInitList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn field_list(&self) -> Option<FieldInitList> {
        self.syntax()
            .children()
            .filter_map(FieldInitList::cast)
            .next()
    }
}

impl FieldInitList {
    /// Iterates direct `FieldInit` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn fields(&self) -> impl Iterator<Item = FieldInit> {
        self.syntax().children().filter_map(FieldInit::cast)
    }
}

impl FieldInit {
    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        self.syntax().children().filter_map(Name::cast).next()
    }

    /// Copies the selected name's spelling, or `None` if the name/token is missing.
    pub fn name_text(&self) -> Option<String> {
        self.name().and_then(|name| name.text())
    }

    /// Returns the value expression: the first direct `Expr` child after filtering.
    /// Returns `None` when this form/component is absent or lost during recovery.
    pub fn value(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }
}

impl MatchExpr {
    /// Returns the match scrutinee: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn scrutinee(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).next()
    }

    /// Returns the arm list: the first direct `MatchArmList` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn arms(&self) -> Option<MatchArmList> {
        self.syntax()
            .children()
            .filter_map(MatchArmList::cast)
            .next()
    }
}

impl LoopExpr {
    /// Returns the body: the first direct `BlockExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<BlockExpr> {
        support::child(self.syntax())
    }
}

impl ClosureExpr {
    /// Whether this callable explicitly carries the `async` modifier.
    pub fn is_async(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|child| child.kind() == SyntaxKind::AsyncKw)
    }

    /// Iterates parameters inside direct parameter-list nodes in source order.
    /// The `||` form has no list and yields no parameters.
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

    /// Returns the body: the last direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn body(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }
}

impl ClosureParam {
    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns the type annotation or assigned type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}

impl MatchArmList {
    /// Iterates direct `MatchArm` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn arms(&self) -> impl Iterator<Item = MatchArm> {
        self.syntax().children().filter_map(MatchArm::cast)
    }
}

impl MatchArm {
    /// Returns the pattern: the first direct `Pattern` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn pattern(&self) -> Option<Pattern> {
        self.syntax().children().filter_map(Pattern::cast).next()
    }

    /// Returns the contained expression: the last direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        self.syntax().children().filter_map(Expr::cast).last()
    }

    /// Returns the first direct expression only when an `if` token is present.
    /// Without a guard marker, returns `None`; recovery can leave it sharing the result node.
    pub fn guard(&self) -> Option<Expr> {
        if support::token(self.syntax(), SyntaxKind::IfKw).is_some() {
            self.syntax().children().filter_map(Expr::cast).next()
        } else {
            None
        }
    }
}

impl Pattern {
    /// Whether a direct `|` token separates alternatives.
    pub fn is_or(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::Pipe).is_some()
    }

    /// Returns `Some(true)` for `..=`, `Some(false)` for `..`, or `None` for no range token.
    pub fn range_inclusive(&self) -> Option<bool> {
        if support::token(self.syntax(), SyntaxKind::DotDotEq).is_some() {
            Some(true)
        } else if support::token(self.syntax(), SyntaxKind::DotDot).is_some() {
            Some(false)
        } else {
            None
        }
    }

    /// Iterates direct literal/path children in source order.
    /// Also yields those children for non-range patterns; check `range_inclusive()` first.
    pub fn range_bounds(&self) -> impl Iterator<Item = PatternBound> {
        self.syntax().children().filter_map(|child| {
            Literal::cast(child.clone())
                .map(PatternBound::Literal)
                .or_else(|| PathExpr::cast(child).map(PatternBound::Path))
        })
    }

    /// Iterates direct `Pattern` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn elements(&self) -> impl Iterator<Item = Pattern> {
        self.syntax().children().filter_map(Pattern::cast)
    }

    /// Whether the first direct path spells `_`.
    pub fn is_wildcard(&self) -> bool {
        self.path().and_then(|path| path.name_text()).as_deref() == Some("_")
    }

    /// Returns the path: the first direct `PathExpr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn path(&self) -> Option<PathExpr> {
        self.syntax().children().filter_map(PathExpr::cast).next()
    }

    /// Returns the literal: the first direct `Literal` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn literal(&self) -> Option<Literal> {
        self.syntax().children().filter_map(Literal::cast).next()
    }

    /// Whether a direct `{` token marks a struct-shaped pattern.
    pub fn is_struct(&self) -> bool {
        support::token(self.syntax(), SyntaxKind::LBrace).is_some()
    }

    /// Whether a direct path and `(` token are both present.
    pub fn is_tuple_struct(&self) -> bool {
        self.path().is_some() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    /// Whether `(` is present and no direct path is present; includes grouped patterns.
    pub fn is_tuple(&self) -> bool {
        self.path().is_none() && support::token(self.syntax(), SyntaxKind::LParen).is_some()
    }

    /// Whether there is one nested pattern inside parentheses and no comma or path.
    pub fn is_grouped(&self) -> bool {
        self.is_tuple()
            && support::token(self.syntax(), SyntaxKind::Comma).is_none()
            && self.elements().count() == 1
    }

    /// Iterates direct `PatternField` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn fields(&self) -> impl Iterator<Item = PatternField> {
        self.syntax().children().filter_map(PatternField::cast)
    }
}

/// A literal or path selected as a possible pattern range endpoint.
pub enum PatternBound {
    /// The [`Literal`] view; see its node diagram.
    Literal(Literal),
    /// The [`PathExpr`] view; see its node diagram.
    Path(PathExpr),
}

impl PatternField {
    /// Returns the name: the first direct `Name` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn name(&self) -> Option<Name> {
        support::child(self.syntax())
    }

    /// Returns the pattern: the first direct `Pattern` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn pattern(&self) -> Option<Pattern> {
        support::child(self.syntax())
    }
}

impl TupleExpr {
    /// Iterates direct `Expr` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}

impl ArrayExpr {
    /// Whether a direct `;` token marks `[value; count]`.
    pub fn is_repeat(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|part| part.kind() == SyntaxKind::Semi)
    }

    /// Iterates direct `Expr` children in source order, skipping other kinds.
    /// Returns an empty iterator when no matching children remain.
    pub fn elements(&self) -> impl Iterator<Item = Expr> {
        self.syntax().children().filter_map(Expr::cast)
    }
}

impl Interpolation {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Whether a direct `:` marker is present; does not revalidate the full `:?` suffix.
    pub fn debug(&self) -> bool {
        self.syntax()
            .children_with_tokens()
            .any(|element| element.kind() == SyntaxKind::Colon)
    }
}

impl CastExpr {
    /// Returns the contained expression: the first direct `Expr` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn expr(&self) -> Option<Expr> {
        support::child(self.syntax())
    }

    /// Returns the type annotation or assigned type: the first direct `TypeRef` child after filtering.
    /// Returns `None` if no matching child exists; see [`Self`] for its tree and recovery notes.
    pub fn ty(&self) -> Option<TypeRef> {
        support::child(self.syntax())
    }
}
