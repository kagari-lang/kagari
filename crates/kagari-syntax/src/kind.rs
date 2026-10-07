//! Kinds shared by CST nodes and tokens; AST accessors select by these tags.
//!
//! Lexer kinds map here through [`crate::token::TokenKind::to_syntax_kind`].
//! Shift operators are combined by the parser; declaration/expression kinds
//! describe grouping nodes and have no single lexer token.

/// The tag of a concrete syntax node or token, independent of resolved meaning.
///
/// Token variants give their spelling; node variants link to their typed views.
/// Raw Rowan kinds must be valid discriminants of this enum.
#[repr(u16)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SyntaxKind {
    /// Reserved placeholder kind; the current lexer and parser do not emit it.
    Tombstone,
    /// End-of-input marker with empty text and a zero-width range at the source end.
    Eof,
    /// Whitespace, including spaces, tabs and line breaks.
    Whitespace,
    /// A `// ...` comment, including `///` and `//!` documentation comments.
    LineComment,
    /// A `/* ... */` comment, which may contain nested block comments.
    BlockComment,
    /// An identifier, such as `total` or `_`.
    Ident,
    /// An integer literal spelling, such as `42` or `0xff`; not a decoded number.
    Number,
    /// A floating-point literal spelling, such as `1.5`.
    Float,
    /// A quoted string literal, such as `"hello"`, retaining quotes and escapes.
    String,
    /// The `f"` prefix opening an interpolated string.
    FormatStart,
    /// Text within an interpolated string, including escaped or doubled braces.
    FormatText,
    /// The `{` opening an expression hole in an interpolated string.
    FormatOpen,
    /// The `}` closing an expression hole, distinct from a nested block brace.
    FormatClose,
    /// The `"` closing an interpolated string.
    FormatEnd,
    /// The `as` keyword.
    AsKw,
    /// The `crate` keyword.
    CrateKw,
    /// The `for` keyword.
    ForKw,
    /// The `in` keyword.
    InKw,
    /// The `fn` keyword.
    FnKw,
    /// The `impl` keyword.
    ImplKw,
    /// The `mod` keyword.
    ModKw,
    /// The `pub` keyword.
    PubKw,
    /// The `self` keyword.
    SelfKw,
    /// The `super` keyword.
    SuperKw,
    /// The `trait` keyword.
    TraitKw,
    /// The `type` keyword.
    TypeKw,
    /// The `use` keyword.
    UseKw,
    /// The `where` keyword.
    WhereKw,
    /// The `const` keyword.
    ConstKw,
    /// The `val` keyword.
    ValKw,
    /// The `var` keyword.
    VarKw,
    /// The `struct` keyword.
    StructKw,
    /// The `enum` keyword.
    EnumKw,
    /// The `return` keyword.
    ReturnKw,
    /// The `if` keyword.
    IfKw,
    /// The `else` keyword.
    ElseKw,
    /// The `match` keyword.
    MatchKw,
    /// The `while` keyword.
    WhileKw,
    /// The `loop` keyword.
    LoopKw,
    /// The `break` keyword.
    BreakKw,
    /// The `continue` keyword.
    ContinueKw,
    /// The `true` keyword.
    TrueKw,
    /// The `false` keyword.
    FalseKw,
    /// The `(` punctuation or operator token.
    LParen,
    /// The `)` punctuation or operator token.
    RParen,
    /// The `[` punctuation or operator token.
    LBracket,
    /// The `]` punctuation or operator token.
    RBracket,
    /// The `{` punctuation or operator token.
    LBrace,
    /// The `}` punctuation or operator token.
    RBrace,
    /// The `,` punctuation or operator token.
    Comma,
    /// The `:` punctuation or operator token.
    Colon,
    /// The `::` punctuation or operator token.
    ColonColon,
    /// The `;` punctuation or operator token.
    Semi,
    /// The `.` punctuation or operator token.
    Dot,
    /// The `..` punctuation or operator token.
    DotDot,
    /// The `..=` punctuation or operator token.
    DotDotEq,
    /// The `#` punctuation or operator token.
    Hash,
    /// The `=` punctuation or operator token.
    Eq,
    /// The `+` punctuation or operator token.
    Plus,
    /// The `+=` punctuation or operator token.
    PlusEq,
    /// The `-` punctuation or operator token.
    Minus,
    /// The `-=` punctuation or operator token.
    MinusEq,
    /// The `*` punctuation or operator token.
    Star,
    /// The `*=` punctuation or operator token.
    StarEq,
    /// The `/` punctuation or operator token.
    Slash,
    /// The `/=` punctuation or operator token.
    SlashEq,
    /// The `%` punctuation or operator token.
    Percent,
    /// The `%=` punctuation or operator token.
    PercentEq,
    /// The `!` punctuation or operator token.
    Bang,
    /// The `?` punctuation or operator token.
    Question,
    /// The `==` punctuation or operator token.
    EqEq,
    /// The `!=` punctuation or operator token.
    NotEq,
    /// The `===` punctuation or operator token.
    IdentityEq,
    /// The `!==` punctuation or operator token.
    IdentityNotEq,
    /// The `<` punctuation or operator token.
    Lt,
    /// The `>` punctuation or operator token.
    Gt,
    /// The `<=` punctuation or operator token.
    Le,
    /// The `>=` punctuation or operator token.
    Ge,
    /// The `<<` punctuation or operator token. Combined from adjacent lexer tokens by the parser.
    Shl,
    /// The `>>` punctuation or operator token. Combined from adjacent lexer tokens by the parser.
    Shr,
    /// The `<<=` punctuation or operator token. Combined from adjacent lexer tokens by the parser.
    ShlEq,
    /// The `>>=` punctuation or operator token. Combined from adjacent lexer tokens by the parser.
    ShrEq,
    /// The `&` punctuation or operator token.
    Amp,
    /// The `&=` punctuation or operator token.
    AmpEq,
    /// The `|=` punctuation or operator token.
    PipeEq,
    /// The `^` punctuation or operator token.
    Caret,
    /// The `^=` punctuation or operator token.
    CaretEq,
    /// The `&&` punctuation or operator token.
    AmpAmp,
    /// The `|` punctuation or operator token.
    Pipe,
    /// The `||` punctuation or operator token.
    PipePipe,
    /// The `->` punctuation or operator token.
    Arrow,
    /// The `=>` punctuation or operator token.
    FatArrow,
    /// Unrecognized or malformed lexical input retained with its source range.
    Unknown,
    /// The root syntax node for one parsed source text. See [`SourceFile`](crate::ast::item::SourceFile).
    SourceFile,
    /// An inline `mod name { ... }` or external `mod name;` declaration. See [`ModuleDef`](crate::ast::item::ModuleDef).
    ModuleDef,
    /// The braced declarations of an inline module. See [`ModuleBlock`](crate::ast::item::ModuleBlock).
    ModuleBlock,
    /// A `use` declaration with an optional visibility prefix. See [`UseDecl`](crate::ast::item::UseDecl).
    UseDecl,
    /// One branch of a use declaration, retaining its local path prefix. See [`UseTree`](crate::ast::item::UseTree).
    UseTree,
    /// Comma-separated child use trees; enclosing braces belong to the parent. See [`UseTreeList`](crate::ast::item::UseTreeList).
    UseTreeList,
    /// A sequence of names separated by `::`, without resolution or generic arguments. See [`Path`](crate::ast::misc::Path).
    Path,
    /// A trait declaration with direct method, constant and associated-type children. See [`TraitDef`](crate::ast::item::TraitDef).
    TraitDef,
    /// An inherent or trait implementation for a type spelling. See [`ImplBlock`](crate::ast::item::ImplBlock).
    ImplBlock,
    /// A trait or implementation method, with a body or terminating semicolon. See [`MethodDef`](crate::ast::item::MethodDef).
    MethodDef,
    /// Angle-bracketed generic parameter declarations. See [`GenericParamList`](crate::ast::misc::GenericParamList).
    GenericParamList,
    /// One generic parameter name with optional trait bounds. See [`GenericParam`](crate::ast::misc::GenericParam).
    GenericParam,
    /// Angle-bracketed type arguments and associated-type bindings. See [`GenericArgList`](crate::ast::misc::GenericArgList).
    GenericArgList,
    /// The `where` keyword and its ordered predicates. See [`WhereClause`](crate::ast::misc::WhereClause).
    WhereClause,
    /// A type spelling constrained by a list of trait bounds. See [`WherePredicate`](crate::ast::misc::WherePredicate).
    WherePredicate,
    /// One or more trait spellings separated by `+`. See [`TraitBoundList`](crate::ast::misc::TraitBoundList).
    TraitBoundList,
    /// A trait path with generic arguments or callable input/output syntax. See [`TraitRef`](crate::ast::misc::TraitRef).
    TraitRef,
    /// A `type` declaration or definition in a trait, impl or declaration-mode file. See [`AssociatedType`](crate::ast::item::AssociatedType).
    AssociatedType,
    /// An associated-type equality inside generic arguments. See [`AssociatedTypeBinding`](crate::ast::misc::AssociatedTypeBinding).
    AssociatedTypeBinding,
    /// A `<Receiver as Trait>::Member` spelling with optional member arguments. See [`QualifiedType`](crate::ast::ty::QualifiedType).
    QualifiedType,
    /// Comma-separated type spellings; delimiters belong to its parent. See [`TypeList`](crate::ast::misc::TypeList).
    TypeList,
    /// A function declaration and its optional generic/signature components. See [`FnDef`](crate::ast::item::FnDef).
    FnDef,
    /// A constant declaration; associated constants can omit their initializer. See [`ConstDef`](crate::ast::item::ConstDef).
    ConstDef,
    /// A nominal struct declaration with named fields. See [`StructDef`](crate::ast::item::StructDef).
    StructDef,
    /// A nominal enum declaration with unit or tuple-payload variants. See [`EnumDef`](crate::ast::item::EnumDef).
    EnumDef,
    /// One identifier or special path segment, without name resolution. See [`Name`](crate::ast::misc::Name).
    Name,
    /// Function or method parameters; parentheses belong to the declaration. See [`ParamList`](crate::ast::misc::ParamList).
    ParamList,
    /// One named parameter, including the special method receiver `self`. See [`Param`](crate::ast::misc::Param).
    Param,
    /// Struct fields in source order; braces belong to the struct declaration. See [`FieldList`](crate::ast::misc::FieldList).
    FieldList,
    /// A struct field with visibility, writeability, name and type spelling. See [`Field`](crate::ast::misc::Field).
    Field,
    /// Enum variants in source order; braces belong to the enum declaration. See [`VariantList`](crate::ast::misc::VariantList).
    VariantList,
    /// A unit variant or a variant with parenthesized payload types. See [`Variant`](crate::ast::misc::Variant).
    Variant,
    /// A type spelling wrapper, not a resolved semantic type. See [`TypeRef`](crate::ast::ty::TypeRef).
    TypeRef,
    /// A tuple type, distinguished from grouping by a comma or empty parentheses. See [`TupleType`](crate::ast::ty::TupleType).
    TupleType,
    /// An array type spelling with one element-type child. See [`ArrayType`](crate::ast::ty::ArrayType).
    ArrayType,
    /// A braced sequence of statements with an optional trailing expression. See [`BlockExpr`](crate::ast::expr::BlockExpr).
    BlockExpr,
    /// A `val` or `var` local declaration with optional type and initializer. See [`BindingStmt`](crate::ast::stmt::BindingStmt).
    BindingStmt,
    /// A `return` statement with an optional result expression. See [`ReturnStmt`](crate::ast::stmt::ReturnStmt).
    ReturnStmt,
    /// An assignment or compound assignment to a syntactic target. See [`AssignStmt`](crate::ast::stmt::AssignStmt).
    AssignStmt,
    /// A while loop with an expression or binding condition. See [`WhileStmt`](crate::ast::stmt::WhileStmt).
    WhileStmt,
    /// A `loop` parsed directly as a block statement. See [`LoopStmt`](crate::ast::stmt::LoopStmt).
    LoopStmt,
    /// A `loop` used in expression position, such as a binding initializer. See [`LoopExpr`](crate::ast::expr::LoopExpr).
    LoopExpr,
    /// A closure parameter list followed by its expression body. See [`ClosureExpr`](crate::ast::expr::ClosureExpr).
    ClosureExpr,
    /// The parameters between a closure's pipes. See [`ClosureParamList`](crate::ast::expr::ClosureParamList).
    ClosureParamList,
    /// A closure parameter with an optional type annotation. See [`ClosureParam`](crate::ast::expr::ClosureParam).
    ClosureParam,
    /// A `fn(...) -> ...` type with separate input list and result. See [`FunctionType`](crate::ast::ty::FunctionType).
    FunctionType,
    /// A pattern binding, iterable expression and loop body. See [`ForStmt`](crate::ast::stmt::ForStmt).
    ForStmt,
    /// A `break` statement with an optional loop result expression. See [`BreakStmt`](crate::ast::stmt::BreakStmt).
    BreakStmt,
    /// A `continue;` statement with no value child. See [`ContinueStmt`](crate::ast::stmt::ContinueStmt).
    ContinueStmt,
    /// An expression consumed as a statement rather than a block tail. See [`ExprStmt`](crate::ast::stmt::ExprStmt).
    ExprStmt,
    /// An unresolved expression path or qualified member spelling. See [`PathExpr`](crate::ast::expr::PathExpr).
    PathExpr,
    /// A literal token, retaining its spelling rather than a decoded value. See [`Literal`](crate::ast::expr::Literal).
    Literal,
    /// A parenthesized expression that preserves explicit grouping. See [`ParenExpr`](crate::ast::expr::ParenExpr).
    ParenExpr,
    /// A unary `-` or `!` expression. See [`PrefixExpr`](crate::ast::expr::PrefixExpr).
    PrefixExpr,
    /// An `as` expression with a target type spelling. See [`CastExpr`](crate::ast::expr::CastExpr).
    CastExpr,
    /// A postfix `?` expression; propagation semantics are checked in HIR. See [`PropagateExpr`](crate::ast::expr::PropagateExpr).
    PropagateExpr,
    /// Two operands joined by an arithmetic, comparison, bitwise or logical operator. See [`BinaryExpr`](crate::ast::expr::BinaryExpr).
    BinaryExpr,
    /// An exclusive `..` or inclusive `..=` range with optional endpoints. See [`RangeExpr`](crate::ast::expr::RangeExpr).
    RangeExpr,
    /// A `val pattern = expression` condition in `if` or `while`. See [`BindingCondition`](crate::ast::expr::BindingCondition).
    BindingCondition,
    /// A `#[...]` declaration annotation. See [`Attribute`](crate::ast::item::Attribute).
    Attribute,
    /// Parenthesized arguments of an attribute. See [`AttributeArgs`](crate::ast::item::AttributeArgs).
    AttributeArgs,
    /// Comma-separated attribute arguments or array elements. See [`AttributeArgList`](crate::ast::item::AttributeArgList).
    AttributeArgList,
    /// One positional or named attribute argument. See [`AttributeArg`](crate::ast::item::AttributeArg).
    AttributeArg,
    /// A literal, path or array inside an attribute. See [`AttributeValue`](crate::ast::item::AttributeValue).
    AttributeValue,
    /// A callee followed by arguments, with an optional generic argument list. See [`CallExpr`](crate::ast::expr::CallExpr).
    CallExpr,
    /// A named member access on a receiver expression. See [`FieldExpr`](crate::ast::expr::FieldExpr).
    FieldExpr,
    /// An indexed access with a receiver and index expression. See [`IndexExpr`](crate::ast::expr::IndexExpr).
    IndexExpr,
    /// A conditional with an expression or binding condition and optional else branch. See [`IfExpr`](crate::ast::expr::IfExpr).
    IfExpr,
    /// A constructor path followed by named field initializers. See [`StructExpr`](crate::ast::expr::StructExpr).
    StructExpr,
    /// The field initializer list of a struct expression. See [`FieldInitList`](crate::ast::expr::FieldInitList).
    FieldInitList,
    /// One named field initializer, with an optional explicit value. See [`FieldInit`](crate::ast::expr::FieldInit).
    FieldInit,
    /// A scrutinee followed by a list of pattern arms. See [`MatchExpr`](crate::ast::expr::MatchExpr).
    MatchExpr,
    /// The list of match arms; enclosing braces belong to the match expression. See [`MatchArmList`](crate::ast::expr::MatchArmList).
    MatchArmList,
    /// A pattern, optional `if` guard, and result expression. See [`MatchArm`](crate::ast::expr::MatchArm).
    MatchArm,
    /// A syntactic pattern; binding and constructor meanings are resolved later. See [`Pattern`](crate::ast::expr::Pattern).
    Pattern,
    /// A named struct-pattern field with an optional explicit subpattern. See [`PatternField`](crate::ast::expr::PatternField).
    PatternField,
    /// A comma-separated tuple expression, including the empty tuple. See [`TupleExpr`](crate::ast::expr::TupleExpr).
    TupleExpr,
    /// An array list or repeated-element array expression. See [`ArrayExpr`](crate::ast::expr::ArrayExpr).
    ArrayExpr,
    /// An `f"..."` string containing text tokens and expression holes. See [`InterpolatedString`](crate::ast::expr::InterpolatedString).
    InterpolatedString,
    /// One expression hole, optionally requesting debug formatting with `:?`. See [`Interpolation`](crate::ast::expr::Interpolation).
    Interpolation,
    /// A recovery node retaining unexpected tokens or a limit-exhausted suffix.
    Error,
}

impl SyntaxKind {
    /// Whether this kind is whitespace or a line/block comment.
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}
