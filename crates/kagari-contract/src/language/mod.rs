//! Portable identities of language protocols. Complete declarations live in the shared catalog.
//! Recognition uses nominal identity; an application trait with the same name
//! never acquires syntax or implicit-value semantics.
use kagari_common::identity::{
    DefinitionKind, DefinitionPath, DefinitionPathSegment, ModuleIdentity, PackageId,
};

use kagari_common::identity::{reference::DefinitionReference, table::DefinitionTable};
pub mod catalog;
pub mod primitive;
pub mod role;

/// Public source spelling; the portable package identity is independent.
pub const SOURCE_PACKAGE: &str = "core";

pub fn module_identity() -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("kagari-core".into()),
        path: vec!["language".into()],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    List,
    MutableList,
    Map,
    MutableMap,
    Set,
    MutableSet,
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
    RangeBounds,
    Debug,
    Display,
    /// Error conversion used by Result propagation.
    From,
    Into,
    TryFrom,
    TryInto,
    FromStr,
    FromIterator,
    Sum,
    Product,
}

pub fn identity(protocol: Protocol) -> DefinitionPath {
    DefinitionPath {
        module: module_identity(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: protocol.name().into(),
            occurrence: 0,
        }],
    }
}

impl Protocol {
    pub const ALL: [Self; 38] = [
        Self::List,
        Self::MutableList,
        Self::Map,
        Self::MutableMap,
        Self::Set,
        Self::MutableSet,
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
        Self::RangeBounds,
        Self::Debug,
        Self::Display,
        Self::From,
        Self::Into,
        Self::TryFrom,
        Self::TryInto,
        Self::FromStr,
        Self::FromIterator,
        Self::Sum,
        Self::Product,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::MutableList => "MutableList",
            Self::Map => "Map",
            Self::MutableMap => "MutableMap",
            Self::Set => "Set",
            Self::MutableSet => "MutableSet",
            Self::Iterator => "Iterator",
            Self::Iterable => "Iterable",
            Self::PartialEq => "PartialEq",
            Self::Eq => "Eq",
            Self::Hash => "Hash",
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
            Self::Fn => "Fn",
            Self::RangeBounds => "RangeBounds",
            Self::Debug => "Debug",
            Self::Display => "Display",
            Self::From => "From",
            Self::Into => "Into",
            Self::TryFrom => "TryFrom",
            Self::TryInto => "TryInto",
            Self::FromStr => "FromStr",
            Self::FromIterator => "FromIterator",
            Self::Sum => "Sum",
            Self::Product => "Product",
        }
    }

    pub fn from_id(id: &DefinitionPath) -> Option<Self> {
        Self::from_reference(id, None)
    }

    pub fn from_reference<I: DefinitionReference>(
        id: &I,
        table: Option<&DefinitionTable>,
    ) -> Option<Self> {
        let view = id.describe(table).ok()?;
        if view.module() != &module_identity() || view.segments().count() != 1 {
            return None;
        }
        let part = view.last()?;
        if part.kind != DefinitionKind::Trait || part.occurrence != 0 {
            return None;
        }
        Self::ALL.into_iter().find(|kind| kind.name() == part.name)
    }

    pub fn conversion(self) -> bool {
        matches!(
            self,
            Self::From | Self::Into | Self::TryFrom | Self::TryInto
        )
    }

    /// Reverse conversion contracts are derived from the destination's impl.
    pub fn conversion_origin(self) -> Option<Self> {
        match self {
            Self::Into => Some(Self::From),
            Self::TryInto => Some(Self::TryFrom),
            _ => None,
        }
    }

    pub fn dynamic(self) -> bool {
        self.collection() || matches!(self, Self::Index | Self::Iterable | Self::Iterator)
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
