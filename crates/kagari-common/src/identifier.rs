//! Identifier spellings shared by the lexer and source-free declaration contracts.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keyword {
    As,
    Async,
    Await,
    Break,
    Const,
    Continue,
    Crate,
    Else,
    Enum,
    False,
    Fn,
    For,
    If,
    Impl,
    In,
    Loop,
    Match,
    Mod,
    Native,
    Pub,
    Return,
    SelfValue,
    Static,
    Struct,
    Super,
    Trait,
    True,
    Type,
    Use,
    Val,
    Var,
    Where,
    While,
}

impl Keyword {
    pub fn from_spelling(word: &str) -> Option<Self> {
        Some(match word {
            "as" => Self::As,
            "async" => Self::Async,
            "await" => Self::Await,
            "break" => Self::Break,
            "const" => Self::Const,
            "continue" => Self::Continue,
            "crate" => Self::Crate,
            "else" => Self::Else,
            "enum" => Self::Enum,
            "false" => Self::False,
            "fn" => Self::Fn,
            "for" => Self::For,
            "if" => Self::If,
            "impl" => Self::Impl,
            "in" => Self::In,
            "loop" => Self::Loop,
            "match" => Self::Match,
            "mod" => Self::Mod,
            "native" => Self::Native,
            "pub" => Self::Pub,
            "return" => Self::Return,
            "self" => Self::SelfValue,
            "static" => Self::Static,
            "struct" => Self::Struct,
            "super" => Self::Super,
            "trait" => Self::Trait,
            "true" => Self::True,
            "type" => Self::Type,
            "use" => Self::Use,
            "val" => Self::Val,
            "var" => Self::Var,
            "where" => Self::Where,
            "while" => Self::While,
            _ => return None,
        })
    }
}

/// Whether the entire spelling is one ordinary Kagari identifier, not a keyword.
pub fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b == b'_' || b.is_ascii_alphabetic())
        && bytes.all(|b| b == b'_' || b.is_ascii_alphanumeric())
        && Keyword::from_spelling(name).is_none()
}
