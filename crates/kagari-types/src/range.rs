//! Syntax-independent identities for the six standard range forms.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum RangeKind {
    Exclusive,
    Inclusive,
    From,
    To,
    ToInclusive,
    Full,
}

impl RangeKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Exclusive => "Range",
            Self::Inclusive => "RangeInclusive",
            Self::From => "RangeFrom",
            Self::To => "RangeTo",
            Self::ToInclusive => "RangeToInclusive",
            Self::Full => "RangeFull",
        }
    }

    pub fn has_start(self) -> bool {
        matches!(self, Self::Exclusive | Self::Inclusive | Self::From)
    }

    pub fn has_end(self) -> bool {
        matches!(
            self,
            Self::Exclusive | Self::Inclusive | Self::To | Self::ToInclusive
        )
    }

    pub fn inclusive(self) -> bool {
        matches!(self, Self::Inclusive | Self::ToInclusive)
    }

    pub fn from_parts(start: bool, end: bool, inclusive: bool) -> Option<Self> {
        Some(match (start, end, inclusive) {
            (true, true, false) => Self::Exclusive,
            (true, true, true) => Self::Inclusive,
            (true, false, false) => Self::From,
            (false, true, false) => Self::To,
            (false, true, true) => Self::ToInclusive,
            (false, false, false) => Self::Full,
            _ => return None,
        })
    }
}
