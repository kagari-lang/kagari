//! Portable identities of language protocols. Complete declarations live in the shared catalog.
//! Recognition uses nominal identity; an application trait with the same name
//! never acquires syntax or implicit-value semantics.
use kagari_common::identity::{
    DefinitionId, DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
};

pub mod catalog;
pub mod primitive;

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
}

pub fn identity(protocol: Protocol) -> DefinitionId {
    DefinitionId {
        module: module_identity(),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: protocol.name().into(),
            occurrence: 0,
        }],
    }
}

impl Protocol {
    pub const ALL: [Self; 31] = [
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
        }
    }
    pub fn from_id(id: &DefinitionId) -> Option<Self> {
        if id.module != module_identity() || id.path.len() != 1 {
            return None;
        }
        Self::ALL.into_iter().find(|kind| identity(*kind) == *id)
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
