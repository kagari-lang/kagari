//! Portable identities of language protocols. Complete declarations live in the shared catalog.
//! Recognition uses nominal identity; an application trait with the same name
//! never acquires syntax or implicit-value semantics.
use kagari_common::identity::{DefinitionKind, DefinitionPath, DefinitionPathSegment};
use kagari_common::identity::{
    ModuleIdentity, PackageId, reference::DefinitionReference, table::DefinitionTable,
};

pub mod adapter;
pub mod binding;
pub mod role;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
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
    /// Error conversion used by Result propagation.
    From,
}

pub fn identity(protocol: Protocol) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity {
            package: PackageId("kagari-core".into()),
            path: vec![
                match protocol {
                    Protocol::Iterator | Protocol::Iterable => "iter",
                    Protocol::PartialEq | Protocol::Eq | Protocol::PartialOrd | Protocol::Ord => {
                        "cmp"
                    }
                    Protocol::Hash => "hash",
                    Protocol::Debug | Protocol::Display => "fmt",
                    Protocol::From => "convert",
                    _ => "ops",
                }
                .into(),
            ],
        },
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: protocol.name().into(),
            occurrence: 0,
        }],
    }
}

impl Protocol {
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
            Self::Debug => "Debug",
            Self::Display => "Display",
            Self::From => "From",
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
        if view.segments().count() != 1 {
            return None;
        }
        let part = view.last()?;
        if part.kind != DefinitionKind::Trait || part.occurrence != 0 {
            return None;
        }
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == part.name && view.module() == &identity(*kind).module)
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

/// Only canonical reserved-role owners can provide language foundation source.
pub fn is_language_module(module: &ModuleIdentity) -> bool {
    Protocol::ALL
        .into_iter()
        .any(|kind| identity(kind).module == *module)
}
