//! Literal payloads: decoded String text and deferred scalar spellings.

/// Literal category and payload retained inline in an expression or pattern.
///
/// | Source | Stored fields |
/// | --- | --- |
/// | `42i32` | `Literal { kind: Number, text: "42i32" }` |
/// | `1.5` | `Literal { kind: Float, text: "1.5" }` |
/// | `"hello"` or `r"hello"` | `Literal { kind: String, text: "hello" }` |
/// | `true` | `Literal { kind: Bool, text: "true" }` |
/// | `'a'` | `Literal { kind: Char, text: "'a'" }` |
///
/// For String, `text` is decoded exactly once at the syntax boundary, including
/// interpolation text chunks. Never decode or re-quote it downstream. Other kinds
/// retain source delimiters/suffixes for scalar checking. `-1` is a Prefix(Neg)
/// expression around a positive literal, not a separate literal category.
#[derive(Debug, Clone)]
pub struct Literal {
    /// Category used to select scalar validation and type inference.
    pub kind: LiteralKind,
    /// Decoded content for String; original token spelling for every other kind.
    pub text: String,
}

/// Lexical category selecting later decoding, not a resolved type.
///
/// `42`/`42i32` use Number, `1.5` uses Float, `"hello"` uses String and
/// `true`/`false` use Bool. Number does not by itself choose i32: suffix/context
/// and type checking determine the semantic type. See Literal's source-to-field
/// table for the payload convention; String text is already decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiteralKind {
    /// An integer-like number token, such as `42` or a suffixed integer.
    Number,
    /// A floating-point token, such as `1.5`.
    Float,
    /// Decoded cooked/raw String content or interpolation text, without delimiters.
    String,
    /// `true` or `false`.
    Bool,
    /// A single-quoted Unicode scalar, such as `'a'` or `'🙂'`.
    Char,
}
