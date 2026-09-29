//! Stable identities and capabilities of standard trait contracts.
use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StandardTrait {
    Map,
    MutableMap,
    Set,
    MutableSet,
    List,
    MutableList,
    RangeBounds,
    PartialEq,
    Eq,
    Hash,
    Debug,
    Display,
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
    FromStr,
    From,
    Into,
    TryFrom,
    TryInto,
    Iterator,
    Iterable,
    FromIterator,
    Sum,
    Product,
    Fn,
}

pub fn identity(kind: StandardTrait) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity {
            package: PackageId("kagari-std".into()),
            path: vec![kind.namespace().into()],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: kind.name().into(),
            occurrence: 0,
        }],
    }
}
impl StandardTrait {
    pub const ALL: [Self; 38] = [
        Self::Map,
        Self::MutableMap,
        Self::Set,
        Self::MutableSet,
        Self::List,
        Self::MutableList,
        Self::RangeBounds,
        Self::PartialEq,
        Self::Eq,
        Self::Hash,
        Self::Debug,
        Self::Display,
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
        Self::FromStr,
        Self::From,
        Self::Into,
        Self::TryFrom,
        Self::TryInto,
        Self::Iterator,
        Self::Iterable,
        Self::FromIterator,
        Self::Sum,
        Self::Product,
        Self::Fn,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Map => "Map",
            Self::MutableMap => "MutableMap",
            Self::Set => "Set",
            Self::MutableSet => "MutableSet",
            Self::List => "List",
            Self::MutableList => "MutableList",
            Self::RangeBounds => "RangeBounds",
            Self::PartialEq => "PartialEq",
            Self::Eq => "Eq",
            Self::Hash => "Hash",
            Self::Debug => "Debug",
            Self::Display => "Display",
            Self::PartialOrd => "PartialOrd",
            Self::Ord => "Ord",
            Self::Add => "Add",
            Self::Sub => "Sub",
            Self::Mul => "Mul",
            Self::Div => "Div",
            Self::Rem => "Rem",
            Self::BitAnd => "BitAnd",
            Self::BitOr => "BitOr",
            Self::BitXor => "BitXor",
            Self::Shl => "Shl",
            Self::Shr => "Shr",

            Self::Neg => "Neg",
            Self::Not => "Not",
            Self::Index => "Index",
            Self::FromStr => "FromStr",
            Self::From => "From",
            Self::Into => "Into",
            Self::TryFrom => "TryFrom",
            Self::TryInto => "TryInto",
            Self::Iterator => "Iterator",
            Self::Iterable => "Iterable",
            Self::FromIterator => "FromIterator",
            Self::Sum => "Sum",
            Self::Product => "Product",
            Self::Fn => "Fn",
        }
    }

    pub fn namespace(self) -> &'static str {
        match self {
            Self::List | Self::MutableList => "array",
            Self::Map | Self::MutableMap => "map",
            Self::Set | Self::MutableSet => "set",
            Self::PartialEq | Self::Eq | Self::PartialOrd | Self::Ord => "cmp",
            Self::Fn
            | Self::RangeBounds
            | Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::Shl
            | Self::Shr
            | Self::Neg
            | Self::Not
            | Self::Index => "ops",
            Self::FromStr => "string",
            Self::From | Self::Into | Self::TryFrom | Self::TryInto => "convert",
            Self::Iterator | Self::Iterable | Self::FromIterator | Self::Sum | Self::Product => {
                "iter"
            }
            Self::Hash => "hash",
            Self::Debug | Self::Display => "fmt",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| {
            name == kind.name() || name == format!("std::{}::{}", kind.namespace(), kind.name())
        })
    }

    pub fn from_id(id: &DefinitionId) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| &identity(*kind) == id)
    }

    pub fn dynamic(self) -> bool {
        self.collection() || matches!(self, Self::Index | Self::Iterable)
    }

    pub fn collection(self) -> bool {
        matches!(
            self,
            Self::List
                | Self::MutableList
                | Self::Map
                | Self::MutableMap
                | Self::Set
                | Self::MutableSet
        )
    }

    pub fn iteration(self) -> bool {
        matches!(self, Self::Iterator | Self::Iterable)
    }

    pub fn aggregation(self) -> bool {
        matches!(self, Self::Sum | Self::Product)
    }

    pub fn conversion(self) -> bool {
        matches!(
            self,
            Self::From | Self::Into | Self::TryFrom | Self::TryInto
        )
    }

    pub fn reverse_conversion(self) -> bool {
        matches!(self, Self::Into | Self::TryInto)
    }

    pub fn fallible_conversion(self) -> bool {
        matches!(self, Self::TryFrom | Self::TryInto)
    }

    pub fn binary_operator(self) -> bool {
        matches!(
            self,
            Self::Add
                | Self::Sub
                | Self::Mul
                | Self::Div
                | Self::Rem
                | Self::BitAnd
                | Self::BitOr
                | Self::BitXor
                | Self::Shl
                | Self::Shr
        )
    }

    pub fn operator(self) -> bool {
        self.binary_operator() || matches!(self, Self::Neg | Self::Not | Self::Index | Self::Fn)
    }

    pub fn host_implementable(self) -> bool {
        matches!(self, Self::Debug | Self::Display)
    }

    pub fn equality_protocol(self) -> bool {
        matches!(self, Self::PartialEq | Self::Eq | Self::Hash)
    }
}
