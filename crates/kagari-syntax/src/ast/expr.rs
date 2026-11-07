//! Expressions retain authored roles, grouping, operators and interpolation order.
//!
//! ```text
//! f(a,,b,)       CallExpr { callee: f, arguments: [a, missing, b], ... }
//! 1 + 2 * 3      BinaryExpr { lhs: 1, operator: Add, rhs: BinaryExpr(Multiply) }
//! if {} else     IfExpr { condition: missing, then_body: {}, else_clause: missing body }
//! "hi {name}"    InterpolatedString.parts = [Text("hi "), Hole(path=name)]
//! ```
//!
//! Lists retain their punctuation separately from elements. Recursive alternatives
//! own boxes; text/token spellings stay in the retained source. Recovery preserves
//! each named role rather than substituting another matching child.

use crate::ast::{
    misc::{GenericArgList, Name, Path, TypeAnnotation},
    site::{Required, SeparatedList, SyntaxSite},
    stmt::Stmt,
    tokens::{self, OperatorToken, Token},
    ty::{QualifiedType, TypeSyntax},
};

/// One written expression. Recursive alternatives own boxes; literals need only
/// a lexical reference. For `a + f(b)`, `BinaryExpr` stores its two operands and
/// the actual `+` site; the call stores `f` separately from argument `b`.
#[derive(Debug)]
pub enum Expr {
    BlockExpr(Box<BlockExpr>),
    PathExpr(Box<PathExpr>),
    Literal(Literal),
    ParenExpr(Box<ParenExpr>),
    PrefixExpr(Box<PrefixExpr>),
    CastExpr(Box<CastExpr>),
    PropagateExpr(Box<PropagateExpr>),
    AwaitExpr(Box<AwaitExpr>),
    BinaryExpr(Box<BinaryExpr>),
    RangeExpr(Box<RangeExpr>),
    CallExpr(Box<CallExpr>),
    FieldExpr(Box<FieldExpr>),
    IndexExpr(Box<IndexExpr>),
    IfExpr(Box<IfExpr>),
    StructExpr(Box<StructExpr>),
    MatchExpr(Box<MatchExpr>),
    LoopExpr(Box<LoopExpr>),
    ClosureExpr(Box<ClosureExpr>),
    AsyncBlockExpr(Box<AsyncBlockExpr>),
    TupleExpr(Box<TupleExpr>),
    ArrayExpr(Box<ArrayExpr>),
    InterpolatedString(Box<InterpolatedString>),
    Error(SyntaxSite),
}

node!(
    /// A braced sequence of statements and an optional final expression.
    ///
    /// ```text
    /// { val x = 1; x + 2 }
    /// open = '{' token, statements = [BindingStmt], tail = Some(BinaryExpr), close = '}' token
    /// ```
    ///
    /// Present required delimiters are abbreviated. A trailing `x + 2;` instead
    /// belongs to `statements` as an [`ExprStmt`](crate::ast::stmt::ExprStmt).
    /// An unclosed block retains a missing `close`, without consuming another role.
    BlockExpr {
        open: Required<Token<tokens::LBrace>>,
        statements: Vec<Stmt>,
        tail: Option<Expr>,
        close: Required<Token<tokens::RBrace>>,
    }
);

/// A plain, explicitly applied or qualified path used as an expression.
///
/// `value` selects `Plain`; `Box<T>::new` selects `Applied`;
/// `<T as Trait>::member` selects `Qualified`. These forms retain syntax,
/// without deciding which declaration the path names.
#[derive(Debug)]
pub enum PathExpr {
    Plain(Path),
    Applied(Box<AppliedPath>),
    Qualified(Box<QualifiedType>),
}

node!(
    /// A path application with an optional following associated member.
    ///
    /// ```text
    /// Box<T>::new     path = Path(Box), application_separator = None
    ///                 arguments = <T>, member = Some(PathMember('::', new))
    /// Box::<T>::new   application_separator = Some('::' token), other roles as above
    /// ```
    ///
    /// Present required slots are abbreviated. The member separator belongs to
    /// [`PathMember`]; call-site applications are represented by [`CallApplication`].
    AppliedPath {
        path: Required<Path>,
        application_separator: Option<Token<tokens::ColonColon>>,
        arguments: GenericArgList,
        member: Option<PathMember>,
    }
);

group!(
    /// The separator and required name after an applied path.
    ///
    /// In `Box<T>::new`, `separator` holds the `::` token and `name` holds `new`.
    /// The enclosing [`AppliedPath`] owns `Box` and `<T>`.
    PathMember {
        pub separator: Token<tokens::ColonColon>,
        pub name: Required<Name>,
    }
);

/// Raw spelling and category resolve through the owning parse; decoding is source-owned.
///
/// For `42` or `"hello"`, `token` refers to the original literal token,
/// including its suffix or delimiters. This wrapper stores neither the decoded
/// value nor a separately allocated spelling. Strings with holes instead use
/// [`InterpolatedString`] and its ordered parts.
#[derive(Debug)]
pub struct Literal {
    pub(crate) token: Token<tokens::LiteralToken>,
}

node!(
    /// A grouped expression, preserving parentheses separately from its child.
    ///
    /// For `(x + 1)`, `expression` holds a required [`BinaryExpr`], and
    /// `open`/`close` hold the parenthesis tokens. `(x,)` is a [`TupleExpr`] instead.
    ParenExpr {
        open: Token<tokens::LParen>,
        expression: Required<Expr>,
        close: Required<Token<tokens::RParen>>,
    }
);

/// The parsed unary operation: `-x` is `Negate`, `!x` is `Not`.
///
/// The enclosing [`OperatorToken`] retains the actual lexical token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
    Negate,
    Not,
}

node!(
    /// A prefix operator and its required operand.
    ///
    /// `-value` stores `operator.operation = Negate` and a path `expression`;
    /// `!ready` uses `Not`. [`OperatorToken`] retains spelling, and a lone operator
    /// retains a missing operand rather than borrowing the next grammar field.
    PrefixExpr {
        operator: Required<OperatorToken<PrefixOp>>,
        expression: Required<Expr>,
    }
);

/// The parsed binary operation, independent of its lexical storage.
///
/// `a + b` uses `Add`; `a >> b` uses `ShiftRight` while its [`OperatorToken`]
/// retains two `>` tokens. [`BinaryExpr`] nesting represents precedence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    BitAnd,
    BitOr,
    BitXor,
    ShiftLeft,
    ShiftRight,
    And,
    Or,
    Equal,
    NotEqual,
    IdentityEqual,
    IdentityNotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

node!(
    /// A binary operation with explicit left and right child roles.
    ///
    /// ```text
    /// 1 + 2 * 3
    /// lhs = Literal(1), operator.operation = Add
    /// rhs = BinaryExpr { lhs: Literal(2), operator: Multiply, rhs: Literal(3) }
    /// ```
    ///
    /// The sketch abbreviates required slots. Operator tokens retain source spelling;
    /// precedence is represented by nesting, not reconstructed by consumers.
    BinaryExpr {
        lhs: Required<Expr>,
        operator: Required<OperatorToken<BinaryOp>>,
        rhs: Required<Expr>,
    }
);

node!(
    /// An expression followed by an `as` token and required target type.
    ///
    /// `value as i64` maps to `expression = PathExpr(value)`, `keyword = 'as'`
    /// and `ty = Named(i64)`, abbreviating present required children.
    CastExpr {
        expression: Required<Expr>,
        keyword: Token<tokens::AsKw>,
        ty: Required<TypeSyntax>,
    }
);

node!(
    /// A postfix propagation expression retaining its question-mark token.
    ///
    /// `read()?` stores a required [`CallExpr`] in `expression` and `?` in
    /// `question`; Result/Option eligibility is checked after parsing.
    PropagateExpr {
        expression: Required<Expr>,
        question: Token<tokens::Question>,
    }
);

node!(
    /// A postfix await with separate dot and keyword tokens.
    ///
    /// `task.await` stores the `task` path in `expression`, `.` in `dot`,
    /// and `await` in the required `keyword` slot.
    AwaitExpr {
        expression: Required<Expr>,
        dot: Token<tokens::Dot>,
        keyword: Required<Token<tokens::AwaitKw>>,
    }
);

/// The written range operator: `..` is `Exclusive`, `..=` is `Inclusive`.
///
/// [`RangeExpr`] and [`RangePattern`] share these spellings but own different
/// endpoint requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    Exclusive,
    Inclusive,
}

node!(
    /// A range expression with independently represented endpoints.
    ///
    /// ```text
    /// 1..       start = Some(Literal(1)), operator = Exclusive, end = None
    /// ..=3      start = None, operator = Inclusive, end = Some(Present(Literal(3)))
    /// 1..=      start = Some(Literal(1)), operator = Inclusive, end = Some(Missing)
    /// ```
    ///
    /// Operators retain their lexical spelling. An absent exclusive end is distinct
    /// from a missing required inclusive end; see [`Required`].
    RangeExpr {
        start: Option<Expr>,
        operator: OperatorToken<RangeOp>,
        end: Option<Required<Expr>>,
    }
);

node!(
    /// A callee, optional type application and parenthesized value arguments.
    ///
    /// ```text
    /// f::<T>(a, b)
    /// callee = PathExpr(f), application = Some(CallApplication('::', <T>))
    /// open = '(' token, arguments = [PathExpr(a), PathExpr(b)], close = ')' token
    /// ```
    ///
    /// Present required slots and list entries are abbreviated. Type arguments belong
    /// to `application`, value arguments to `arguments`. [`SeparatedList`] retains
    /// commas and missing entries even when convenience accessors filter them out.
    CallExpr {
        callee: Required<Expr>,
        application: Option<Box<CallApplication>>,
        open: Required<Token<tokens::LParen>>,
        arguments: SeparatedList<Expr>,
        close: Required<Token<tokens::RParen>>,
    }
);

group!(
    /// A call's explicit type arguments and optional application separator.
    ///
    /// For `f::<T>(x)`, `separator` holds the `::` token and `arguments` owns
    /// `<T>`; the enclosing [`CallExpr`] owns the value argument list `(x)`.
    CallApplication {
        pub separator: Option<Token<tokens::ColonColon>>,
        pub arguments: GenericArgList,
    }
);

node!(
    /// A receiver and field name separated by a dot token.
    ///
    /// `point.x` stores a path `receiver`, the `.` token and `name = Name(x)`.
    /// `point.` retains a missing required name; receiver syntax remains intact.
    FieldExpr {
        receiver: Required<Expr>,
        dot: Token<tokens::Dot>,
        name: Required<Name>,
    }
);

node!(
    /// An indexed receiver with bracket tokens and a required index expression.
    ///
    /// `items[i + 1]` maps `receiver` to the `items` path and `index` to a
    /// [`BinaryExpr`]; `open`/`close` retain `[`/`]`. Bounds semantics belong to analysis/runtime.
    IndexExpr {
        receiver: Required<Expr>,
        open: Token<tokens::LBracket>,
        index: Required<Expr>,
        close: Required<Token<tokens::RBracket>>,
    }
);

node!(
    /// An if expression with distinct condition, then-body and optional else roles.
    ///
    /// ```text
    /// if ready { yes } else { no }
    /// keyword = 'if' token, condition = Expression(PathExpr(ready))
    /// then_body = BlockExpr(tail = yes), else_clause = Some(ElseClause('else', BlockExpr(no)))
    /// ```
    ///
    /// Present required slots are abbreviated. `if {}` has a missing condition
    /// and a present body; `if { ready } { yes }` retains the first block as its
    /// condition. A written `else` at EOF retains a missing required body,
    /// whereas no else clause is `None`.
    IfExpr {
        keyword: Token<tokens::IfKw>,
        condition: Required<Condition>,
        then_body: Required<Box<BlockExpr>>,
        else_clause: Option<ElseClause>,
    }
);

/// An ordinary expression or a conditional pattern binding.
///
/// `if ready {}` uses `Expression`; `if val Some(x) = value {}` uses
/// `Binding`. The enclosing `if` or `while` node owns the body separately.
#[derive(Debug)]
pub enum Condition {
    Expression(Expr),
    Binding(Box<BindingCondition>),
}

node!(
    /// A conditional `val` binding with explicit pattern and initializer roles.
    ///
    /// In `if val Some(x) = value {}`, this node owns the `val` and `=` tokens,
    /// `pattern = TupleStruct(Some(x))` and `initializer = PathExpr(value)`.
    /// The enclosing control-flow node owns its keyword and body.
    BindingCondition {
        keyword: Token<tokens::ValKw>,
        pattern: Required<Pattern>,
        equal: Required<Token<tokens::Eq>>,
        initializer: Required<Expr>,
    }
);

group!(
    /// An else keyword with a required expression, including nested `if`.
    ///
    /// `else { value }` stores a block in `body`; `else if ready {}` stores an
    /// [`IfExpr`]. A written `else` at EOF retains the keyword and a missing body.
    ElseClause {
        pub keyword: Token<tokens::ElseKw>,
        pub body: Required<Expr>,
    }
);

node!(
    /// A constructor path, optional type arguments and named field initializers.
    ///
    /// For `Point<T> { x: value }`, `path` is `Plain(Point)`, `arguments` stores
    /// `<T>`, and `fields` owns the brace-delimited [`FieldInitList`].
    /// Present required children are abbreviated; path resolution is deferred.
    StructExpr {
        path: Required<PathExpr>,
        arguments: Option<Box<GenericArgList>>,
        fields: FieldInitList,
    }
);

node!(
    /// A braced, comma-preserving list of fields and an optional final update.
    ///
    /// For `{ x: 1, y, ..base }`, `entries` contains two `Field` alternatives and
    /// one `Update`; `open`/`close` hold brace tokens. Entries after an update are
    /// recovered unexpected content, not additional admitted fields. For
    /// `{ ..base, x: 1 }`, the update has no separator; the rejected comma and
    /// field belong to `entries` at insertion index 1 with `RecoveryPart::Element`.
    FieldInitList {
        open: Required<Token<tokens::LBrace>>,
        entries: SeparatedList<StructEntry>,
        close: Required<Token<tokens::RBrace>>,
    }
);

/// A named field initializer or final struct-update base.
///
/// In `Point { x: 1, ..base }`, entries are `Field(FieldInit)` then
/// `Update(StructUpdateBase)`; list commas belong to [`FieldInitList`].
#[derive(Debug)]
pub enum StructEntry {
    Field(FieldInit),
    Update(StructUpdateBase),
}

node!(
    /// A named initializer, distinguishing shorthand from an explicit value.
    ///
    /// ```text
    /// x          name = Name(x), value = None
    /// x: next    name = Name(x), value = Some(FieldValue(':' token, PathExpr(next)))
    /// ```
    ///
    /// Present required names are abbreviated. `x:` retains a present value clause
    /// with a missing expression; it does not become shorthand.
    FieldInit {
        name: Required<Name>,
        value: Option<FieldValue>,
    }
);

group!(
    /// The colon token and required expression of an explicit field initializer.
    ///
    /// In `x: next`, `colon` holds `:` and `expression` holds `PathExpr(next)`;
    /// the enclosing [`FieldInit`] owns the name `x`.
    FieldValue {
        pub colon: Token<tokens::Colon>,
        pub expression: Required<Expr>,
    }
);

node!(
    /// A struct update entry with a dots token and required base expression.
    ///
    /// `Point { ..base }` stores `dots = '..'` and `value = PathExpr(base)` in
    /// this entry; the enclosing [`FieldInitList`] owns braces and any comma.
    StructUpdateBase {
        dots: Token<tokens::DotDot>,
        value: Required<Expr>,
    }
);

node!(
    /// A match keyword, required scrutinee and arm list.
    ///
    /// For `match value { Some(x) => x, _ => 0 }`, `scrutinee` is the `value`
    /// path and `arms` owns two [`MatchArm`] nodes plus the brace tokens.
    MatchExpr {
        keyword: Token<tokens::MatchKw>,
        scrutinee: Required<Expr>,
        arms: MatchArmList,
    }
);

node!(
    /// The braces and comma-preserving arms of a match expression.
    ///
    /// For `{ A => 1, B => 2 }`, `arms` contains two [`MatchArm`] entries.
    /// Missing arms or separators remain visible through [`SeparatedList`].
    MatchArmList {
        open: Required<Token<tokens::LBrace>>,
        arms: SeparatedList<MatchArm>,
        close: Required<Token<tokens::RBrace>>,
    }
);

node!(
    /// A pattern, optional guard and result expression separated by an arrow.
    ///
    /// ```text
    /// Some(x) if ready => x
    /// pattern = TupleStruct(Some(x)), guard = Some(MatchGuard('if', ready))
    /// arrow = '=>' token, expression = PathExpr(x)
    /// ```
    ///
    /// Present required slots are abbreviated. The enclosing [`MatchArmList`] owns
    /// any trailing comma, while a written `if` with no expression retains its guard.
    MatchArm {
        pattern: Required<Pattern>,
        guard: Option<MatchGuard>,
        arrow: Required<Token<tokens::FatArrow>>,
        expression: Required<Expr>,
    }
);

group!(
    /// The `if` token and required expression guarding a match arm.
    ///
    /// In `x if ready => x`, `keyword` holds `if` and `expression` holds the
    /// `ready` path. The surrounding [`MatchArm`] owns the pattern and arrow.
    MatchGuard {
        pub keyword: Token<tokens::IfKw>,
        pub expression: Required<Expr>,
    }
);

node!(
    /// An expression-form loop with a keyword token and required block.
    ///
    /// In `val x = loop { break 1; };`, `keyword` holds `loop` and `body`
    /// holds a [`BlockExpr`] containing the break statement.
    LoopExpr {
        keyword: Token<tokens::LoopKw>,
        body: Required<BlockExpr>,
    }
);

node!(
    /// An async keyword followed by a required block expression.
    ///
    /// `async { work().await }` stores `async` in `keyword` and the block in
    /// `body`; capture and Future construction semantics belong to later phases.
    AsyncBlockExpr {
        keyword: Token<tokens::AsyncKw>,
        body: Required<BlockExpr>,
    }
);

node!(
    /// A closure's optional async token, parameter form and required body.
    ///
    /// ```text
    /// |x: i32| x + 1    async_keyword = None, parameters = List(x: i32), body = BinaryExpr
    /// async || { 1 }    async_keyword = Some('async'), parameters = Empty('||'), body = BlockExpr
    /// ```
    ///
    /// Present required slots are abbreviated. The body may be any admitted
    /// expression; capture decisions are not stored in this syntax node.
    ClosureExpr {
        async_keyword: Option<Token<tokens::AsyncKw>>,
        parameters: Required<ClosureParams>,
        body: Required<Expr>,
    }
);

/// The compact empty closure delimiter or a pipe-delimited parameter list.
///
/// `|| 1` retains one `PipePipe` token in `Empty`; `|x| x` retains a
/// [`ClosureParamList`] with two delimiter tokens in `List`.
#[derive(Debug)]
pub enum ClosureParams {
    Empty(Token<tokens::PipePipe>),
    List(ClosureParamList),
}

node!(
    /// Pipe-delimited closure parameters with retained commas.
    ///
    /// For `|x, y: i32|`, `open`/`close` hold pipe tokens and `parameters`
    /// contains two [`ClosureParam`] entries. The compact `||` form uses
    /// [`ClosureParams::Empty`] instead.
    ClosureParamList {
        open: Token<tokens::Pipe>,
        parameters: SeparatedList<ClosureParam>,
        close: Required<Token<tokens::Pipe>>,
    }
);

node!(
    /// A closure parameter name with an optional type annotation.
    ///
    /// `x` has `annotation = None`; `x: i32` has `Some(TypeAnnotation)`.
    /// The required `name` retains its own token site, independent of the type.
    ClosureParam {
        name: Required<Name>,
        annotation: Option<Box<TypeAnnotation>>,
    }
);

node!(
    /// A parenthesized, comma-preserving tuple expression.
    ///
    /// `(x,)` has one `elements` entry with a trailing comma; `()` has none.
    /// `open`/`close` are parenthesis tokens. `(x)` is [`ParenExpr`] instead;
    /// see [`SeparatedList`] for the stored entry and separator representation.
    TupleExpr {
        open: Token<tokens::LParen>,
        elements: SeparatedList<Expr>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A bracketed array with element-list or repeat contents.
    ///
    /// `[a, b]` stores `contents = List(...)`; `[value; count]` stores `Repeat`.
    /// `open`/`close` retain bracket tokens, and [`ArrayContents`] owns the contents'
    /// children and internal punctuation.
    ArrayExpr {
        open: Token<tokens::LBracket>,
        contents: ArrayContents,
        close: Required<Token<tokens::RBracket>>,
    }
);

/// Array elements or the distinct value/count roles of a repeat form.
///
/// `[a, b]` uses a comma-preserving `List`; `[x; n]` uses `Repeat` with
/// required `value = x`, semicolon `separator` and required `count = n`.
/// The enclosing [`ArrayExpr`] owns the brackets.
#[derive(Debug)]
pub enum ArrayContents {
    List(SeparatedList<Expr>),
    Repeat {
        value: Required<Expr>,
        separator: Token<tokens::Semi>,
        count: Required<Expr>,
    },
}

node!(
    /// An interpolated string with ordered text tokens and hole nodes.
    ///
    /// ```text
    /// "hi {name}"
    /// open = FormatStart token, parts = [Text("hi " token), Hole(Interpolation(name))]
    /// close = FormatEnd token
    /// ```
    ///
    /// Present required slots are abbreviated. Text remains source-backed, with
    /// escapes and doubled braces undecoded; [`Interpolation`] owns each hole's
    /// delimiters and optional debug marker. Raw/byte strings and cooked strings
    /// without holes instead use [`Literal`].
    InterpolatedString {
        open: Token<tokens::FormatStart>,
        parts: Vec<StringPart>,
        close: Required<Token<tokens::FormatEnd>>,
    }
);

/// A source-backed text token or an interpolation node, in authored order.
///
/// For `"a{name}b"`, parts are `Text`, `Hole`, `Text`. Hole delimiters belong
/// to [`Interpolation`]; quote delimiters belong to [`InterpolatedString`].
#[derive(Debug)]
pub enum StringPart {
    Text(Token<tokens::FormatText>),
    Hole(Interpolation),
}

node!(
    /// A string hole containing a required path and optional debug marker.
    ///
    /// For `"{name:?}"`, `open`/`close` hold hole delimiter tokens, `expression`
    /// holds `PathExpr(name)`, and `debug` is `Some(DebugMarker)`. The child is a
    /// path expression, not an unrestricted [`Expr`]; an absent marker is `None`.
    /// The lexer admits only a single non-keyword ASCII name, not a qualified path.
    /// For `"{a.b}"`, `expression` is missing and `a.b` is unexpected input owned
    /// by that field. For `"{name"`, the name is present and `close` is missing;
    /// the quote still belongs to the enclosing string.
    Interpolation {
        open: Token<tokens::FormatOpen>,
        expression: Required<PathExpr>,
        debug: Option<DebugMarker>,
        close: Required<Token<tokens::FormatClose>>,
    }
);

group!(
    /// The colon and required question mark of a debug-format hole suffix.
    ///
    /// In `"{name:?}"`, `colon` holds `:` and `question` holds `?`;
    /// the enclosing [`Interpolation`] owns `name` and the hole delimiters.
    DebugMarker {
        pub colon: Token<tokens::Colon>,
        pub question: Required<Token<tokens::Question>>,
    }
);

/// Explicit pattern alternatives; expression/place classification is not parser policy.
///
/// `_` is a wildcard token, `Some(x)` is a tuple-struct node, and
/// `Point { x, .. }` is a struct-pattern node. `(x)` preserves grouping while
/// `(x,)` preserves a tuple and its comma. `Error` retains source coverage;
/// binding names and constructor paths are resolved after parsing.
#[derive(Debug)]
pub enum Pattern {
    Wildcard(Token<tokens::NameToken>),
    Path(Box<PathExpr>),
    Literal(Literal),
    Or(Box<OrPattern>),
    Range(Box<RangePattern>),
    Grouped(Box<GroupedPattern>),
    Tuple(Box<TuplePattern>),
    TupleStruct(Box<TupleStructPattern>),
    Struct(Box<StructPattern>),
    Error(SyntaxSite),
}

node!(
    /// An initial pattern followed by pipe-prefixed alternatives.
    ///
    /// For `A | B | C`, `first` holds `A` and `alternatives` has two
    /// [`PatternAlternative`] values for `| B` and `| C`. A final bare pipe
    /// retains an alternative with a missing required pattern.
    OrPattern {
        first: Required<Pattern>,
        alternatives: Vec<PatternAlternative>,
    }
);

group!(
    /// A pipe token and required next pattern in an alternative chain.
    ///
    /// In `A | B`, `separator` holds `|` and `pattern` holds `B`;
    /// the enclosing [`OrPattern`] owns the initial `A`.
    PatternAlternative {
        pub separator: Token<tokens::Pipe>,
        pub pattern: Required<Pattern>,
    }
);

node!(
    /// A bounded pattern range with required endpoints and an operator.
    ///
    /// `1..=3` stores literal `start`/`end` bounds and an `Inclusive` operator;
    /// `LOW..HIGH` stores path bounds and `Exclusive`. Unlike [`RangeExpr`],
    /// both endpoints are required grammar roles.
    RangePattern {
        start: Required<PatternBound>,
        operator: OperatorToken<RangeOp>,
        end: Required<PatternBound>,
    }
);

/// A literal or path endpoint of a bounded pattern range.
///
/// `1..=MAX` stores `Literal(1)` and `Path(MAX)` in the enclosing
/// [`RangePattern`]; endpoints are not unrestricted expressions.
#[derive(Debug)]
pub enum PatternBound {
    Literal(Literal),
    Path(PathExpr),
}

node!(
    /// Parentheses around one pattern, distinct from a tuple pattern.
    ///
    /// `(A | B)` stores an [`OrPattern`] in `pattern` with separate
    /// `open`/`close` tokens; `(x,)` instead uses [`TuplePattern`].
    GroupedPattern {
        open: Token<tokens::LParen>,
        pattern: Required<Pattern>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A parenthesized list of patterns with retained comma positions.
    ///
    /// `(x, _)` stores path and wildcard entries in `elements`; `(x,)` stores
    /// one entry and its trailing comma. `open`/`close` retain parentheses.
    TuplePattern {
        open: Token<tokens::LParen>,
        elements: SeparatedList<Pattern>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A constructor path followed by parenthesized payload patterns.
    ///
    /// `Some(x)` stores the `Some` path in `path`, one pattern in `elements`,
    /// and separate `open`/`close` tokens. The constructor path is not a payload entry.
    TupleStructPattern {
        path: PathExpr,
        open: Token<tokens::LParen>,
        elements: SeparatedList<Pattern>,
        close: Required<Token<tokens::RParen>>,
    }
);

node!(
    /// A constructor path and named pattern fields with an optional rest marker.
    ///
    /// ```text
    /// Point { x, y: other, .. }
    /// path = Plain(Point), fields = [PatternField(x), PatternField(y: other)]
    /// rest = Some(PatternRest('..')), open = '{' token, close = '}' token
    /// ```
    ///
    /// The sketch abbreviates present required slots and list entries. `rest` is
    /// separate from the comma-preserving `fields` list; it is not an ordinary field.
    StructPattern {
        path: PathExpr,
        open: Token<tokens::LBrace>,
        fields: SeparatedList<PatternField>,
        rest: Option<PatternRest>,
        close: Required<Token<tokens::RBrace>>,
    }
);

group!(
    /// A struct pattern's dots token and optional following comma.
    ///
    /// `Point { .., }` has `dots = '..'` and `comma = Some(',')`.
    /// The surrounding [`StructPattern`] owns the closing brace.
    PatternRest {
        pub dots: Token<tokens::DotDot>,
        pub comma: Option<Token<tokens::Comma>>,
    }
);

node!(
    /// A named field pattern with shorthand or an explicit nested pattern.
    ///
    /// ```text
    /// x          name = Name(x), pattern = None
    /// x: other   name = Name(x), pattern = Some(FieldPattern(':' token, Path(other)))
    /// ```
    ///
    /// Present required names are abbreviated. A written `x:` retains a present
    /// clause with a missing required pattern, distinct from shorthand.
    PatternField {
        name: Required<Name>,
        pattern: Option<FieldPattern>,
    }
);

group!(
    /// The colon token and required nested pattern after a field name.
    ///
    /// In `Point { x: other }`, `colon` holds `:` and `pattern` holds `other`;
    /// the enclosing [`PatternField`] owns `x`.
    FieldPattern {
        pub colon: Token<tokens::Colon>,
        pub pattern: Required<Pattern>,
    }
);
