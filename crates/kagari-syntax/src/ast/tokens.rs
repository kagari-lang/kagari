//! Typed, spelling-free references into the owning parse's single lexical table.
//!
//! Markers identify a fixed spelling, such as [`Plus`], or a grammar category,
//! such as [`LiteralToken`]. The parser admits the category before constructing
//! a token; markers add no stored tag or runtime validation.

use std::{marker::PhantomData, num::NonZeroU32};

use crate::{
    ast::{
        expr::{BinaryOp, PrefixOp, RangeOp},
        stmt::AssignmentOp,
    },
    kind::SyntaxKind,
};

/// A present lexical token of grammatical category `K`; construction is parser-owned.
/// `K` is a grammar-role marker, not stored spelling. A copied token does not retain
/// its parse or source; resolve it through its original [`Ast`](crate::ast::borrow::Ast)
/// context. Missing required punctuation uses [`Required::Missing`](crate::ast::site::Required::Missing)
/// rather than a synthetic token.
#[derive(Debug)]
pub struct Token<K> {
    index: NonZeroU32,
    marker: PhantomData<K>,
}

impl<K> Token<K> {
    pub(crate) fn new(index: u32) -> Self {
        Self {
            index: NonZeroU32::new(index.checked_add(1).expect("present lexical index"))
                .expect("one-based lexical index"),
            marker: PhantomData,
        }
    }

    pub(crate) fn index(self) -> u32 {
        self.index.get() - 1
    }
}

impl<K> Copy for Token<K> {}

impl<K> Clone for Token<K> {
    fn clone(&self) -> Self {
        *self
    }
}

/// An operator's parsed alternative and actual contiguous lexical pieces.
/// For example, `ShiftRight` retains both `>` tokens in `a >> b`. The interpreted
/// operation and lexical spelling are separate; source text is resolved through
/// the owning parse by [`OperatorRef`](crate::ast::visit::OperatorRef).
#[derive(Debug)]
pub struct OperatorToken<O> {
    pub operation: O,
    pub(crate) first: u32,
    pub(crate) end: u32,
}

pub(crate) trait OperatorKind {
    fn syntax_kind(&self) -> SyntaxKind;
}

impl OperatorKind for BinaryOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Add => SyntaxKind::Plus,
            Self::Subtract => SyntaxKind::Minus,
            Self::Multiply => SyntaxKind::Star,
            Self::Divide => SyntaxKind::Slash,
            Self::Remainder => SyntaxKind::Percent,
            Self::BitAnd => SyntaxKind::Amp,
            Self::BitOr => SyntaxKind::Pipe,
            Self::BitXor => SyntaxKind::Caret,
            Self::ShiftLeft => SyntaxKind::Shl,
            Self::ShiftRight => SyntaxKind::Shr,
            Self::And => SyntaxKind::AmpAmp,
            Self::Or => SyntaxKind::PipePipe,
            Self::Equal => SyntaxKind::EqEq,
            Self::NotEqual => SyntaxKind::NotEq,
            Self::IdentityEqual => SyntaxKind::IdentityEq,
            Self::IdentityNotEqual => SyntaxKind::IdentityNotEq,
            Self::Less => SyntaxKind::Lt,
            Self::LessEqual => SyntaxKind::Le,
            Self::Greater => SyntaxKind::Gt,
            Self::GreaterEqual => SyntaxKind::Ge,
        }
    }
}

impl OperatorKind for PrefixOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Negate => SyntaxKind::Minus,
            Self::Not => SyntaxKind::Bang,
        }
    }
}

impl OperatorKind for RangeOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Exclusive => SyntaxKind::DotDot,
            Self::Inclusive => SyntaxKind::DotDotEq,
        }
    }
}

impl OperatorKind for AssignmentOp {
    fn syntax_kind(&self) -> SyntaxKind {
        match self {
            Self::Assign => SyntaxKind::Eq,
            Self::Compound(operation) => match operation {
                BinaryOp::Add => SyntaxKind::PlusEq,
                BinaryOp::Subtract => SyntaxKind::MinusEq,
                BinaryOp::Multiply => SyntaxKind::StarEq,
                BinaryOp::Divide => SyntaxKind::SlashEq,
                BinaryOp::Remainder => SyntaxKind::PercentEq,
                BinaryOp::BitAnd => SyntaxKind::AmpEq,
                BinaryOp::BitOr => SyntaxKind::PipeEq,
                BinaryOp::BitXor => SyntaxKind::CaretEq,
                BinaryOp::ShiftLeft => SyntaxKind::ShlEq,
                BinaryOp::ShiftRight => SyntaxKind::ShrEq,
                _ => operation.syntax_kind(),
            },
        }
    }
}

macro_rules! markers {
    ($($(#[$meta:meta])* $name:ident),+ $(,)?) => {
        $(
            $(#[$meta])*
            #[derive(Debug)]
            pub enum $name {}
        )+
    };
}

markers!(
    /// A numeric, String, char, byte, byte-string or boolean literal token.
    /// Admits `Number`, `Float`, `String`, `Char`, `ByteChar`, `ByteString`,
    /// `TrueKw` and `FalseKw` from [`TokenKind`](crate::token::TokenKind).
    /// For example, `42` occupies `Literal.token`; interpolated strings instead
    /// have their own AST nodes and format tokens. Decoding remains source-owned.
    LiteralToken,

    /// The `val` or `var` keyword of a binding (`ValKw` or `VarKw`).
    /// For `var count = 0`, `BindingKeyword.token` refers to `var`, while
    /// [`Writeability`](crate::ast::misc::Writeability) records the parsed choice.
    BindingKeywordToken,

    /// The `crate` or `super` keyword in a visibility restriction (`CrateKw` or `SuperKw`).
    /// For `pub(crate)`, `VisibilityRestriction.scope` refers only to `crate`;
    /// the enclosing node stores the parentheses separately.
    VisibilityScopeToken,

    /// The `+` token, including the separator in `T: Read + Write`.
    Plus,

    /// The `*` token, including the wildcard in `use module::*`.
    Star,

    /// The single `||` token used for an empty closure parameter list in `|| 42`.
    PipePipe,

    /// An identifier or a keyword admitted in a name position.
    /// Admits `Ident` (including `_`), `CrateKw`, `SelfKw` and `SuperKw`;
    /// each grammar position decides which of these is allowed. For example,
    /// `super::value` stores two names, and a receiver parameter stores `self`.
    NameToken,

    /// The `,` token separating list entries, including a trailing comma.
    Comma,

    /// The `:` token introducing an annotation, bound, field value or debug format.
    Colon,

    /// The `::` token separating path segments or introducing a path application.
    ColonColon,

    /// The `;` token terminating a statement/declaration or separating `[value; count]`.
    Semi,

    /// The `=` token introducing an initializer, alias target or associated type binding.
    Eq,

    /// The `->` token introducing a return type.
    Arrow,

    /// The `=>` token separating a match arm's pattern/guard from its body.
    FatArrow,

    /// The `(` token opening a parenthesized construct or parameter/argument list.
    LParen,

    /// The `)` token closing a parenthesized construct or parameter/argument list.
    RParen,

    /// The `[` token opening an array, index expression or attribute body.
    LBracket,

    /// The `]` token closing an array, index expression or attribute body.
    RBracket,

    /// The `{` token opening a braced construct; interpolation uses [`FormatOpen`].
    LBrace,

    /// The `}` token closing a braced construct; interpolation uses [`FormatClose`].
    RBrace,

    /// The single `<` token opening generic parameters/arguments or a qualified path.
    Lt,

    /// The single `>` token closing generic parameters/arguments or a qualified path.
    Gt,

    /// The single `|` token delimiting closure parameters or separating pattern alternatives.
    Pipe,

    /// The `.` token introducing member access, including `.await`.
    Dot,

    /// The `..` token, including a range, rest pattern or struct update prefix.
    DotDot,

    /// The `..=` token spelling an inclusive range operator.
    DotDotEq,

    /// The `#` token introducing an attribute such as `#[meta]`.
    Hash,

    /// The `?` token used for propagation or the debug format marker in `"{value:?}"`.
    Question,

    /// The `!` token, including the never type spelling.
    Bang,

    /// The `as` keyword in a cast, import alias or qualified path.
    AsKw,

    /// The `async` keyword marking an async function, closure or block.
    AsyncKw,

    /// The `await` keyword in a postfix `.await` expression.
    AwaitKw,

    /// The `fn` keyword introducing a function declaration or function type.
    FnKw,

    /// The `mod` keyword introducing a module declaration.
    ModKw,

    /// The `use` keyword introducing an import declaration.
    UseKw,

    /// The `pub` keyword introducing visibility, optionally followed by a restriction.
    PubKw,

    /// The `struct` keyword introducing a struct declaration.
    StructKw,

    /// The `enum` keyword introducing an enum declaration.
    EnumKw,

    /// The `trait` keyword introducing a trait declaration.
    TraitKw,

    /// The `impl` keyword introducing an implementation block.
    ImplKw,

    /// The `for` keyword in a loop or trait implementation header.
    ForKw,

    /// The `in` keyword separating a for-loop pattern from its iterable expression.
    InKw,

    /// The `type` keyword introducing a type alias or associated type declaration.
    TypeKw,

    /// The `const` keyword introducing a constant declaration.
    ConstKw,

    /// The `static` keyword introducing a static declaration.
    StaticKw,

    /// The `where` keyword introducing generic constraints.
    WhereKw,

    /// The `if` keyword introducing a conditional expression or match guard.
    IfKw,

    /// The `else` keyword introducing an alternative conditional branch.
    ElseKw,

    /// The `match` keyword introducing a match expression.
    MatchKw,

    /// The `loop` keyword introducing an unconditional loop.
    LoopKw,

    /// The `while` keyword introducing a conditional loop.
    WhileKw,

    /// The `return` keyword introducing a return statement.
    ReturnKw,

    /// The `break` keyword introducing a loop exit statement.
    BreakKw,

    /// The `continue` keyword introducing a loop continuation statement.
    ContinueKw,

    /// The `val` keyword in a binding condition such as `if val Some(x) = value`.
    /// Ordinary `val`/`var` bindings use [`BindingKeywordToken`].
    ValKw,

    /// The opening `"` of an interpolated String, stored in `InterpolatedString.open`.
    /// A complete String without interpolation is a single [`LiteralToken`] instead.
    FormatStart,

    /// The closing `"` of an interpolated String, stored in `InterpolatedString.close`.
    /// An absent closing quote is represented by `Required::Missing`.
    FormatEnd,

    /// The `{` opening an interpolation hole such as `{name}`, stored in `Interpolation.open`.
    /// A doubled `{{` belongs to [`FormatText`] instead.
    FormatOpen,

    /// The `}` closing an interpolation hole, stored in `Interpolation.close`.
    /// A doubled `}}` in the surrounding string belongs to [`FormatText`] instead.
    FormatClose,

    /// An undecoded source-text segment between interpolation holes or string delimiters.
    /// Stored in `StringPart::Text`; retains written escapes, doubled braces and line endings.
    FormatText,
);
