//! Private keys for the Rust foundation registration inventory. Generic
//! consumers use declared identities/capabilities, never this enum.
use crate::catalog::{assembly_identity, identities};
use kagari_common::identity::{
    DefinitionKind,
    map::DefinitionContext,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::{
    declaration::module::DeclarationError,
    ty::{NominalTy, Ty},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RegistrationTrait {
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
    /// Infallible conversion with identity and lossless scalar implementations.
    From,
    Try,
    FromResidual,
    Into,
    TryFrom,
    TryInto,
    FromStr,
    FromIterator,
    Sum,
    Product,
}

pub(super) fn identity(
    ids: &DefinitionContext,
    protocol: RegistrationTrait,
) -> Result<DefinitionId, DeclarationError> {
    identities::definition(ids, DefinitionKind::Trait, protocol.name())
}

impl RegistrationTrait {
    const ALL: [Self; 40] = [
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
        Self::Try,
        Self::FromResidual,
        Self::Into,
        Self::TryFrom,
        Self::TryInto,
        Self::FromStr,
        Self::FromIterator,
        Self::Sum,
        Self::Product,
    ];

    pub(super) fn name(self) -> &'static str {
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
            Self::Try => "Try",
            Self::FromResidual => "FromResidual",
            Self::Into => "Into",
            Self::TryFrom => "TryFrom",
            Self::TryInto => "TryInto",
            Self::FromStr => "FromStr",
            Self::FromIterator => "FromIterator",
            Self::Sum => "Sum",
            Self::Product => "Product",
        }
    }

    pub(super) fn from_id(id: &DefinitionId, table: &DefinitionTable) -> Option<Self> {
        let view = table.resolve(*id).ok()?;
        if view.module() != &assembly_identity() || view.segments().count() != 1 {
            return None;
        }
        let part = view.last()?;
        if part.kind != DefinitionKind::Trait || part.occurrence != 0 {
            return None;
        }
        Self::ALL.into_iter().find(|kind| kind.name() == part.name)
    }

    pub(super) fn iteration(self) -> bool {
        matches!(self, Self::Iterator | Self::Iterable)
    }
}

pub(super) fn applied(
    ids: &DefinitionContext,
    kind: RegistrationTrait,
    arguments: Vec<Ty>,
) -> Result<NominalTy, DeclarationError> {
    Ok(NominalTy {
        declaration: identity(ids, kind)?,
        arguments,
        associated_types: Default::default(),
    })
}
