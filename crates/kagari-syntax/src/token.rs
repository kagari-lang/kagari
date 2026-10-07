//! Lexical categories and byte ranges before construction of the CST.
//!
//! Tokens contain no owned spelling; slice the original input using their span.
//! Unlike [`SyntaxKind`], this vocabulary contains no tree nodes or combined shift
//! operators. See [`crate::lexer::lex`] for a span-reading example.

use kagari_common::span::Span;

use crate::kind::SyntaxKind;

/// The category of one lexer token; source spelling lives in [`Token::span`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Whitespace, including spaces, tabs and line breaks.
    Whitespace,
    /// A `// ...` comment, including `///` and `//!` documentation comments.
    LineComment,
    /// A `/* ... */` comment, which may contain nested block comments.
    BlockComment,
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
    /// End-of-input marker with empty text and a zero-width range at the source end.
    Eof,
    /// Unrecognized or malformed lexical input retained with its source range.
    Unknown,
}

/// One lexical item, retaining its category and range in the original input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    /// Lexical category, including trivia, malformed input and EOF.
    pub kind: TokenKind,
    /// Half-open UTF-8 byte range `start..end`; EOF is `len..len`.
    pub span: Span,
}

impl TokenKind {
    /// Whether this token is whitespace or a line/block comment.
    pub fn is_trivia(&self) -> bool {
        matches!(
            self,
            Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }

    /// Maps this lexical category to its CST token tag without inspecting text.
    pub fn to_syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Whitespace => SyntaxKind::Whitespace,
            Self::LineComment => SyntaxKind::LineComment,
            Self::BlockComment => SyntaxKind::BlockComment,
            Self::AsKw => SyntaxKind::AsKw,
            Self::CrateKw => SyntaxKind::CrateKw,
            Self::ForKw => SyntaxKind::ForKw,
            Self::InKw => SyntaxKind::InKw,
            Self::FnKw => SyntaxKind::FnKw,
            Self::ImplKw => SyntaxKind::ImplKw,
            Self::ModKw => SyntaxKind::ModKw,
            Self::PubKw => SyntaxKind::PubKw,
            Self::SelfKw => SyntaxKind::SelfKw,
            Self::SuperKw => SyntaxKind::SuperKw,
            Self::TraitKw => SyntaxKind::TraitKw,
            Self::TypeKw => SyntaxKind::TypeKw,
            Self::UseKw => SyntaxKind::UseKw,
            Self::WhereKw => SyntaxKind::WhereKw,
            Self::ConstKw => SyntaxKind::ConstKw,
            Self::ValKw => SyntaxKind::ValKw,
            Self::VarKw => SyntaxKind::VarKw,
            Self::StructKw => SyntaxKind::StructKw,
            Self::EnumKw => SyntaxKind::EnumKw,
            Self::ReturnKw => SyntaxKind::ReturnKw,
            Self::IfKw => SyntaxKind::IfKw,
            Self::ElseKw => SyntaxKind::ElseKw,
            Self::MatchKw => SyntaxKind::MatchKw,
            Self::WhileKw => SyntaxKind::WhileKw,
            Self::LoopKw => SyntaxKind::LoopKw,
            Self::BreakKw => SyntaxKind::BreakKw,
            Self::ContinueKw => SyntaxKind::ContinueKw,
            Self::TrueKw => SyntaxKind::TrueKw,
            Self::FalseKw => SyntaxKind::FalseKw,
            Self::Ident => SyntaxKind::Ident,
            Self::Number => SyntaxKind::Number,
            Self::Float => SyntaxKind::Float,
            Self::String => SyntaxKind::String,
            Self::FormatStart => SyntaxKind::FormatStart,
            Self::FormatText => SyntaxKind::FormatText,
            Self::FormatOpen => SyntaxKind::FormatOpen,
            Self::FormatClose => SyntaxKind::FormatClose,
            Self::FormatEnd => SyntaxKind::FormatEnd,
            Self::LParen => SyntaxKind::LParen,
            Self::RParen => SyntaxKind::RParen,
            Self::LBracket => SyntaxKind::LBracket,
            Self::RBracket => SyntaxKind::RBracket,
            Self::LBrace => SyntaxKind::LBrace,
            Self::RBrace => SyntaxKind::RBrace,
            Self::Comma => SyntaxKind::Comma,
            Self::Colon => SyntaxKind::Colon,
            Self::ColonColon => SyntaxKind::ColonColon,
            Self::Semi => SyntaxKind::Semi,
            Self::Dot => SyntaxKind::Dot,
            Self::DotDot => SyntaxKind::DotDot,
            Self::DotDotEq => SyntaxKind::DotDotEq,
            Self::Hash => SyntaxKind::Hash,
            Self::Eq => SyntaxKind::Eq,
            Self::Plus => SyntaxKind::Plus,
            Self::PlusEq => SyntaxKind::PlusEq,
            Self::Minus => SyntaxKind::Minus,
            Self::MinusEq => SyntaxKind::MinusEq,
            Self::Star => SyntaxKind::Star,
            Self::StarEq => SyntaxKind::StarEq,
            Self::Slash => SyntaxKind::Slash,
            Self::SlashEq => SyntaxKind::SlashEq,
            Self::Percent => SyntaxKind::Percent,
            Self::PercentEq => SyntaxKind::PercentEq,
            Self::Bang => SyntaxKind::Bang,
            Self::Question => SyntaxKind::Question,
            Self::IdentityEq => SyntaxKind::IdentityEq,
            Self::IdentityNotEq => SyntaxKind::IdentityNotEq,
            Self::EqEq => SyntaxKind::EqEq,
            Self::NotEq => SyntaxKind::NotEq,
            Self::Lt => SyntaxKind::Lt,
            Self::Gt => SyntaxKind::Gt,
            Self::Le => SyntaxKind::Le,
            Self::Ge => SyntaxKind::Ge,
            Self::Amp => SyntaxKind::Amp,
            Self::AmpEq => SyntaxKind::AmpEq,
            Self::PipeEq => SyntaxKind::PipeEq,
            Self::Caret => SyntaxKind::Caret,
            Self::CaretEq => SyntaxKind::CaretEq,
            Self::AmpAmp => SyntaxKind::AmpAmp,
            Self::Pipe => SyntaxKind::Pipe,
            Self::PipePipe => SyntaxKind::PipePipe,
            Self::Arrow => SyntaxKind::Arrow,
            Self::FatArrow => SyntaxKind::FatArrow,
            Self::Eof => SyntaxKind::Eof,
            Self::Unknown => SyntaxKind::Unknown,
        }
    }
}
