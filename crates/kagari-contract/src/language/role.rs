//! Reserved language roles. Ordinary library traits have no role.
use crate::language::Protocol;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LangRole {
    Iterator,
    Iterable,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Neg,
    Not,
    Index,
    Fn,
    Debug,
    Display,
    From,
}

impl LangRole {
    pub const ALL: [Self; 24] = [
        Self::Iterator,
        Self::Iterable,
        Self::PartialEq,
        Self::Eq,
        Self::Hash,
        Self::PartialOrd,
        Self::Ord,
        Self::Add,
        Self::Sub,
        Self::Mul,
        Self::Div,
        Self::Rem,
        Self::BitAnd,
        Self::BitOr,
        Self::BitXor,
        Self::Shl,
        Self::Shr,
        Self::Neg,
        Self::Not,
        Self::Index,
        Self::Fn,
        Self::Debug,
        Self::Display,
        Self::From,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Iterator => "iterator",
            Self::Iterable => "iterable",
            Self::PartialEq => "partial_eq",
            Self::Eq => "eq",
            Self::Hash => "hash",
            Self::PartialOrd => "partial_ord",
            Self::Ord => "ord",
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Rem => "rem",
            Self::BitAnd => "bit_and",
            Self::BitOr => "bit_or",
            Self::BitXor => "bit_xor",
            Self::Shl => "shl",
            Self::Shr => "shr",
            Self::Neg => "neg",
            Self::Not => "not",
            Self::Index => "index",
            Self::Fn => "fn",
            Self::Debug => "debug",
            Self::Display => "display",
            Self::From => "from",
        }
    }

    /// Reserved core protocols whose implicit value contracts remain static.
    pub fn requires_static_dispatch(self) -> bool {
        !matches!(self, Self::Index | Self::Iterable | Self::Iterator)
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|role| role.name() == name)
    }

    pub fn protocol(self) -> Protocol {
        match self {
            Self::Iterator => Protocol::Iterator,
            Self::Iterable => Protocol::Iterable,
            Self::PartialEq => Protocol::PartialEq,
            Self::Eq => Protocol::Eq,
            Self::Hash => Protocol::Hash,
            Self::PartialOrd => Protocol::PartialOrd,
            Self::Ord => Protocol::Ord,
            Self::Add => Protocol::Add,
            Self::Sub => Protocol::Sub,
            Self::Mul => Protocol::Mul,
            Self::Div => Protocol::Div,
            Self::Rem => Protocol::Rem,
            Self::BitAnd => Protocol::BitAnd,
            Self::BitOr => Protocol::BitOr,
            Self::BitXor => Protocol::BitXor,
            Self::Shl => Protocol::Shl,
            Self::Shr => Protocol::Shr,
            Self::Neg => Protocol::Neg,
            Self::Not => Protocol::Not,
            Self::Index => Protocol::Index,
            Self::Fn => Protocol::Fn,
            Self::Debug => Protocol::Debug,
            Self::Display => Protocol::Display,
            Self::From => Protocol::From,
        }
    }

    pub fn from_protocol(protocol: Protocol) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|role| role.protocol() == protocol)
    }
}
