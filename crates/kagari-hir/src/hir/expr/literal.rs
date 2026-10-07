//! Literal spellings retained until checked scalar/string interpretation.

/// Literal category and source spelling, not an already evaluated scalar value.
#[derive(Debug, Clone)]
pub struct Literal {
    /// Lexical category used to select later decoding and type inference.
    pub kind: LiteralKind,
    /// Literal spelling, retaining delimiters/suffixes; interpolation can synthesize string spelling.
    pub text: String,
}

/// Lexical literal categories retained by lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    /// An integer-like number token, such as `42` or a suffixed integer.
    Number,
    /// A floating-point token, such as `1.5`.
    Float,
    /// A quoted string literal or synthesized interpolation-text chunk.
    String,
    /// `true` or `false`.
    Bool,
}
