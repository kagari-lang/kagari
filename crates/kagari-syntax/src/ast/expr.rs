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

node!(BlockExpr {
    open: Required<Token<tokens::LBrace>>,
    statements: Vec<Stmt>,
    tail: Option<Expr>,
    close: Required<Token<tokens::RBrace>>,
});

#[derive(Debug)]
pub enum PathExpr {
    Plain(Path),
    Applied(Box<AppliedPath>),
    Qualified(Box<QualifiedType>),
}

node!(AppliedPath {
    path: Required<Path>,
    application_separator: Option<Token<tokens::ColonColon>>,
    arguments: GenericArgList,
    member: Option<PathMember>,
});

group!(PathMember {
    pub separator: Token<tokens::ColonColon>,
    pub name: Required<Name>,
});

/// Raw spelling and category resolve through the owning parse; decoding is source-owned.
#[derive(Debug)]
pub struct Literal {
    pub(crate) token: Token<tokens::Lexeme>,
}

node!(ParenExpr {
    open: Token<tokens::LParen>,
    expression: Required<Expr>,
    close: Required<Token<tokens::RParen>>,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixOp {
    Negate,
    Not,
}

node!(PrefixExpr {
    operator: Required<OperatorToken<PrefixOp>>,
    expression: Required<Expr>,
});

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

node!(BinaryExpr {
    lhs: Required<Expr>,
    operator: Required<OperatorToken<BinaryOp>>,
    rhs: Required<Expr>,
});

node!(CastExpr {
    expression: Required<Expr>,
    keyword: Token<tokens::AsKw>,
    ty: Required<TypeSyntax>,
});

node!(PropagateExpr {
    expression: Required<Expr>,
    question: Token<tokens::Question>,
});

node!(AwaitExpr {
    expression: Required<Expr>,
    dot: Token<tokens::Dot>,
    keyword: Required<Token<tokens::AwaitKw>>,
});

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeOp {
    Exclusive,
    Inclusive,
}

node!(RangeExpr {
    start: Option<Expr>,
    operator: OperatorToken<RangeOp>,
    end: Option<Required<Expr>>,
});

node!(CallExpr {
    callee: Required<Expr>,
    application: Option<Box<CallApplication>>,
    open: Required<Token<tokens::LParen>>,
    arguments: SeparatedList<Expr>,
    close: Required<Token<tokens::RParen>>,
});

group!(CallApplication {
    pub separator: Option<Token<tokens::ColonColon>>,
    pub arguments: GenericArgList,
});

node!(FieldExpr {
    receiver: Required<Expr>,
    dot: Token<tokens::Dot>,
    name: Required<Name>,
});

node!(IndexExpr {
    receiver: Required<Expr>,
    open: Token<tokens::LBracket>,
    index: Required<Expr>,
    close: Required<Token<tokens::RBracket>>,
});

node!(
    /// `if {}` has a missing condition and a present body. `else` remains present
    /// even if its required expression is missing at EOF.
    IfExpr {
    keyword: Token<tokens::IfKw>,
    condition: Required<Condition>,
    then_body: Required<Box<BlockExpr>>,
    else_clause: Option<ElseClause>,
});

#[derive(Debug)]
pub enum Condition {
    Expression(Expr),
    Binding(Box<BindingCondition>),
}

node!(BindingCondition {
    keyword: Token<tokens::ValKw>,
    pattern: Required<Pattern>,
    equal: Required<Token<tokens::Eq>>,
    initializer: Required<Expr>,
});

group!(ElseClause {
    pub keyword: Token<tokens::ElseKw>,
    pub body: Required<Expr>,
});

node!(StructExpr {
    path: Required<PathExpr>,
    arguments: Option<Box<GenericArgList>>,
    fields: FieldInitList,
});

node!(FieldInitList {
    open: Required<Token<tokens::LBrace>>,
    entries: SeparatedList<StructEntry>,
    close: Required<Token<tokens::RBrace>>,
});

#[derive(Debug)]
pub enum StructEntry {
    Field(FieldInit),
    Update(StructUpdateBase),
}

node!(FieldInit {
    name: Required<Name>,
    value: Option<FieldValue>,
});

group!(FieldValue {
    pub colon: Token<tokens::Colon>,
    pub expression: Required<Expr>,
});

node!(StructUpdateBase {
    dots: Token<tokens::DotDot>,
    value: Required<Expr>,
});

node!(MatchExpr {
    keyword: Token<tokens::MatchKw>,
    scrutinee: Required<Expr>,
    arms: MatchArmList,
});

node!(MatchArmList {
    open: Required<Token<tokens::LBrace>>,
    arms: SeparatedList<MatchArm>,
    close: Required<Token<tokens::RBrace>>,
});

node!(MatchArm {
    pattern: Required<Pattern>,
    guard: Option<MatchGuard>,
    arrow: Required<Token<tokens::FatArrow>>,
    expression: Required<Expr>,
});

group!(MatchGuard {
    pub keyword: Token<tokens::IfKw>,
    pub expression: Required<Expr>,
});

node!(LoopExpr {
    keyword: Token<tokens::LoopKw>,
    body: Required<BlockExpr>,
});

node!(AsyncBlockExpr {
    keyword: Token<tokens::AsyncKw>,
    body: Required<BlockExpr>,
});

node!(ClosureExpr {
    async_keyword: Option<Token<tokens::AsyncKw>>,
    parameters: Required<ClosureParams>,
    body: Required<Expr>,
});

#[derive(Debug)]
pub enum ClosureParams {
    Empty(Token<tokens::Lexeme>),
    List(ClosureParamList),
}

node!(ClosureParamList {
    open: Token<tokens::Pipe>,
    parameters: SeparatedList<ClosureParam>,
    close: Required<Token<tokens::Pipe>>,
});

node!(ClosureParam {
    name: Required<Name>,
    annotation: Option<Box<TypeAnnotation>>,
});

node!(TupleExpr {
    open: Token<tokens::LParen>,
    elements: SeparatedList<Expr>,
    close: Required<Token<tokens::RParen>>,
});

node!(ArrayExpr {
    open: Token<tokens::LBracket>,
    contents: ArrayContents,
    close: Required<Token<tokens::RBracket>>,
});

#[derive(Debug)]
pub enum ArrayContents {
    List(SeparatedList<Expr>),
    Repeat {
        value: Required<Expr>,
        separator: Token<tokens::Semi>,
        count: Required<Expr>,
    },
}

node!(InterpolatedString {
    open: Token<tokens::FormatStart>,
    parts: Vec<StringPart>,
    close: Required<Token<tokens::FormatEnd>>,
});

#[derive(Debug)]
pub enum StringPart {
    Text(Token<tokens::FormatText>),
    Hole(Interpolation),
}

node!(Interpolation {
    open: Token<tokens::FormatOpen>,
    expression: Required<PathExpr>,
    debug: Option<DebugMarker>,
    close: Required<Token<tokens::FormatClose>>,
});

group!(DebugMarker {
    pub colon: Token<tokens::Colon>,
    pub question: Required<Token<tokens::Question>>,
});

/// Explicit pattern alternatives; expression/place classification is not parser policy.
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

node!(OrPattern {
    first: Required<Pattern>,
    alternatives: Vec<PatternAlternative>,
});

group!(PatternAlternative {
    pub separator: Token<tokens::Pipe>,
    pub pattern: Required<Pattern>,
});

node!(RangePattern {
    start: Required<PatternBound>,
    operator: OperatorToken<RangeOp>,
    end: Required<PatternBound>,
});

#[derive(Debug)]
pub enum PatternBound {
    Literal(Literal),
    Path(PathExpr),
}

node!(GroupedPattern {
    open: Token<tokens::LParen>,
    pattern: Required<Pattern>,
    close: Required<Token<tokens::RParen>>,
});

node!(TuplePattern {
    open: Token<tokens::LParen>,
    elements: SeparatedList<Pattern>,
    close: Required<Token<tokens::RParen>>,
});

node!(TupleStructPattern {
    path: PathExpr,
    open: Token<tokens::LParen>,
    elements: SeparatedList<Pattern>,
    close: Required<Token<tokens::RParen>>,
});

node!(StructPattern {
    path: PathExpr,
    open: Token<tokens::LBrace>,
    fields: SeparatedList<PatternField>,
    rest: Option<PatternRest>,
    close: Required<Token<tokens::RBrace>>,
});

group!(PatternRest {
    pub dots: Token<tokens::DotDot>,
    pub comma: Option<Token<tokens::Comma>>,
});

node!(PatternField {
    name: Required<Name>,
    pattern: Option<FieldPattern>,
});

group!(FieldPattern {
    pub colon: Token<tokens::Colon>,
    pub pattern: Required<Pattern>,
});
