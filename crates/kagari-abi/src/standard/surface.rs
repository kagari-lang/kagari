//! Closed scalar and native representation facts; no source declaration catalog.
use crate::scalar::BuiltinType;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinTypeFamily {
    Never,
    Unit,
    Boolean,
    SignedInteger,
    UnsignedInteger,
    Float,
    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuiltinTypeSpec {
    pub ty: BuiltinType,
    pub name: &'static str,
    pub family: BuiltinTypeFamily,
    pub const_safe: bool,
    pub heap_backed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum StandardEnum {
    Bound,
    ParseError,
    TryFromIntError,
    Infallible,
    Option,
    Result,
    Ordering,
}

impl StandardEnum {
    /// Generic slots in the engine representation, independently of source names.
    pub fn arity(self) -> usize {
        match self {
            Self::Bound | Self::Option => 1,
            Self::Result => 2,
            Self::ParseError | Self::TryFromIntError | Self::Infallible | Self::Ordering => 0,
        }
    }

    /// Wire discriminant order and payload slots are part of the runtime ABI.
    pub fn variants(self) -> &'static [StandardVariant] {
        match self {
            Self::Bound => &[
                StandardVariant::Included,
                StandardVariant::Excluded,
                StandardVariant::Unbounded,
            ],
            Self::ParseError => &[
                StandardVariant::ParseEmpty,
                StandardVariant::ParseInvalidDigit,
                StandardVariant::ParseOutOfRange,
                StandardVariant::ParseInvalidRadix,
                StandardVariant::ParseInvalidSyntax,
            ],
            Self::TryFromIntError => &[StandardVariant::OutOfRange],
            Self::Infallible => &[],
            Self::Option => &[StandardVariant::Some, StandardVariant::None],
            Self::Result => &[StandardVariant::Ok, StandardVariant::Err],
            Self::Ordering => &[
                StandardVariant::Less,
                StandardVariant::Equal,
                StandardVariant::Greater,
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardVariant {
    ParseEmpty,
    ParseInvalidDigit,
    ParseOutOfRange,
    ParseInvalidRadix,
    ParseInvalidSyntax,

    Included,
    Excluded,
    Unbounded,
    OutOfRange,
    Less,
    Equal,
    Greater,
    Some,
    None,
    Ok,
    Err,
}

impl StandardVariant {
    pub fn kind(self) -> StandardEnum {
        match self {
            Self::ParseEmpty
            | Self::ParseInvalidDigit
            | Self::ParseOutOfRange
            | Self::ParseInvalidRadix
            | Self::ParseInvalidSyntax => StandardEnum::ParseError,
            Self::Included | Self::Excluded | Self::Unbounded => StandardEnum::Bound,
            Self::OutOfRange => StandardEnum::TryFromIntError,
            Self::Less | Self::Equal | Self::Greater => StandardEnum::Ordering,
            Self::Some | Self::None => StandardEnum::Option,
            Self::Ok | Self::Err => StandardEnum::Result,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::ParseEmpty => 0,
            Self::ParseInvalidDigit => 1,
            Self::ParseOutOfRange => 2,
            Self::ParseInvalidRadix => 3,
            Self::ParseInvalidSyntax => 4,

            Self::Included => 0,
            Self::Excluded => 1,
            Self::Unbounded => 2,
            Self::OutOfRange | Self::Less => 0,
            Self::Equal => 1,
            Self::Greater => 2,
            Self::Some | Self::Ok => 0,
            Self::None | Self::Err => 1,
        }
    }

    pub fn payload(self) -> Option<usize> {
        match self {
            Self::ParseEmpty
            | Self::ParseInvalidDigit
            | Self::ParseOutOfRange
            | Self::ParseInvalidRadix
            | Self::ParseInvalidSyntax
            | Self::Unbounded
            | Self::OutOfRange
            | Self::None
            | Self::Less
            | Self::Equal
            | Self::Greater => None,
            Self::Err => Some(1),
            _ => Some(0),
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
// Operation predicates for intrinsic signatures. HashKey and Comparable delegate
// to canonical standard traits and are not source-level bound names.
pub enum StandardTypeConstraint {
    HashKey,
    OrderedNumber,
    SignedNumber,
    Comparable,
}

impl StandardTypeConstraint {
    /// Closed numeric predicates do not consult user trait implementations.
    pub fn accepts_builtin_number(self, ty: BuiltinType) -> bool {
        match self {
            Self::OrderedNumber => ty.number_type().is_some(),
            Self::SignedNumber => {
                ty.integer_layout().is_some_and(|(_, signed)| signed)
                    || matches!(ty, BuiltinType::F32 | BuiltinType::F64)
            }
            Self::HashKey | Self::Comparable => false,
        }
    }

    /// Sealed engine predicates that can be written as source bounds.
    pub fn source_bound_name(self) -> Option<&'static str> {
        match self {
            Self::OrderedNumber => Some("OrderedNumber"),
            Self::SignedNumber => Some("SignedNumber"),
            Self::HashKey | Self::Comparable => None,
        }
    }
}

const BUILTIN_TYPES: &[BuiltinTypeSpec] = &[
    BuiltinTypeSpec {
        ty: BuiltinType::Never,
        name: "!",
        family: BuiltinTypeFamily::Never,
        const_safe: false,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Unit,
        name: "()",
        family: BuiltinTypeFamily::Unit,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::Bool,
        name: "bool",
        family: BuiltinTypeFamily::Boolean,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I8,
        name: "i8",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I16,
        name: "i16",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I32,
        name: "i32",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::I64,
        name: "i64",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::ISize,
        name: "isize",
        family: BuiltinTypeFamily::SignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U8,
        name: "u8",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U16,
        name: "u16",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U32,
        name: "u32",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::U64,
        name: "u64",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::USize,
        name: "usize",
        family: BuiltinTypeFamily::UnsignedInteger,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F32,
        name: "f32",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::F64,
        name: "f64",
        family: BuiltinTypeFamily::Float,
        const_safe: true,
        heap_backed: false,
    },
    BuiltinTypeSpec {
        ty: BuiltinType::String,
        name: "String",
        family: BuiltinTypeFamily::String,
        const_safe: false,
        heap_backed: true,
    },
];

pub fn builtin_types() -> &'static [BuiltinTypeSpec] {
    BUILTIN_TYPES
}

pub fn builtin_type(name: &str) -> Option<BuiltinType> {
    builtin_types()
        .iter()
        .find(|spec| spec.name == name)
        .map(|spec| spec.ty)
}

pub fn builtin_type_spec(ty: BuiltinType) -> Option<&'static BuiltinTypeSpec> {
    builtin_types().iter().find(|spec| spec.ty == ty)
}

pub fn standard_constraint_name(constraint: StandardTypeConstraint) -> &'static str {
    match constraint {
        StandardTypeConstraint::HashKey => "Eq + Hash",
        StandardTypeConstraint::OrderedNumber => "OrderedNumber",
        StandardTypeConstraint::SignedNumber => "SignedNumber",
        StandardTypeConstraint::Comparable => "PartialEq",
    }
}
