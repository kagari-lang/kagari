//! Literal spellings retained until checked scalar/string interpretation.

/// Literal category and spelling retained inline in an expression or pattern.
///
/// | Source | Stored fields |
/// | --- | --- |
/// | `42i32` | `Literal { kind: Number, text: "42i32" }` |
/// | `1.5` | `Literal { kind: Float, text: "1.5" }` |
/// | `"hello"` | `Literal { kind: String, text: "\"hello\"" }` |
/// | `true` | `Literal { kind: Bool, text: "true" }` |
///
/// `text` includes source delimiters/suffixes; it is not a parsed Rust scalar or
/// decoded runtime string. `-1` is a Prefix(Neg) expression around a positive
/// literal, rather than a separate literal category. Interpolation text chunks
/// synthesize quoted spellings. Scalar/string checking decodes and validates
/// the literal, including suffixes, escapes and bounds.
#[derive(Debug, Clone)]
pub struct Literal {
    /// Lexical category used to select later decoding and type inference.
    pub kind: LiteralKind,
    /// Literal spelling, retaining delimiters/suffixes; interpolation can synthesize string spelling.
    pub text: String,
}

/// Lexical category selecting later decoding, not a resolved type.
///
/// `42`/`42i32` use Number, `1.5` uses Float, `"hello"` uses String and
/// `true`/`false` use Bool. Number does not by itself choose i32: suffix/context
/// and type checking determine the semantic type. The matching Literal.text
/// retains the spelling; see Literal's source-to-field table.
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
