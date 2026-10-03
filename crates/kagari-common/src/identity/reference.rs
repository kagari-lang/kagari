//! Representations used by the same semantic metadata at explicit boundaries.
use crate::identity::{
    DefinitionKind, DefinitionPath, ModuleIdentity,
    table::{
        DefinitionId, DefinitionSegment, DefinitionTable, DefinitionTableError, DefinitionView,
        wire::PortableDefinitionRef,
    },
};
use std::{fmt::Debug, hash::Hash};

mod sealed {
    pub trait Sealed {}
}

/// Scoped IDs and local wire references are checked by their owning table. This
/// structural predicate only bounds owned authoring paths; it does not authorize
/// an ID from another context or replace reference resolution during decoding.
pub trait DefinitionReference: sealed::Sealed + Debug + Clone + Eq + Ord + Hash {
    fn within_path_limit(&self) -> bool;

    fn describe<'a>(
        &'a self,
        table: Option<&'a DefinitionTable>,
    ) -> Result<DefinitionDescription<'a>, DefinitionTableError>;

    fn resolve(&self, table: &DefinitionTable) -> Result<DefinitionId, DefinitionTableError>;

    /// Only authoring representations contain an owned path. Contextual and
    /// portable references require explicit resolution by their owning table.
    fn authoring_path(&self) -> Option<&DefinitionPath> {
        None
    }
}

impl sealed::Sealed for DefinitionPath {}

impl DefinitionReference for DefinitionPath {
    fn resolve(&self, table: &DefinitionTable) -> Result<DefinitionId, DefinitionTableError> {
        table
            .lookup(self)
            .ok_or(DefinitionTableError::UnmappedDefinition)
    }

    fn describe<'a>(
        &'a self,
        _: Option<&'a DefinitionTable>,
    ) -> Result<DefinitionDescription<'a>, DefinitionTableError> {
        if !self.within_path_limit() {
            return Err(DefinitionTableError::PathLimit);
        }
        Ok(DefinitionDescription::Path(self))
    }

    fn within_path_limit(&self) -> bool {
        self.within_path_limit()
    }

    fn authoring_path(&self) -> Option<&DefinitionPath> {
        Some(self)
    }
}

impl sealed::Sealed for DefinitionId {}

impl DefinitionReference for DefinitionId {
    fn resolve(&self, table: &DefinitionTable) -> Result<DefinitionId, DefinitionTableError> {
        table.resolve(*self)?;
        Ok(*self)
    }

    fn describe<'a>(
        &'a self,
        table: Option<&'a DefinitionTable>,
    ) -> Result<DefinitionDescription<'a>, DefinitionTableError> {
        Ok(DefinitionDescription::Scoped(
            table
                .ok_or(DefinitionTableError::ForeignTable)?
                .resolve(*self)?,
        ))
    }

    fn within_path_limit(&self) -> bool {
        true
    }
}

impl sealed::Sealed for PortableDefinitionRef {}

impl DefinitionReference for PortableDefinitionRef {
    fn resolve(&self, _: &DefinitionTable) -> Result<DefinitionId, DefinitionTableError> {
        Err(DefinitionTableError::UnmappedDefinition)
    }

    fn describe<'a>(
        &'a self,
        _: Option<&'a DefinitionTable>,
    ) -> Result<DefinitionDescription<'a>, DefinitionTableError> {
        Err(DefinitionTableError::UnmappedDefinition)
    }

    fn within_path_limit(&self) -> bool {
        true
    }
}

/// Borrowed identity content. Portable references must be decoded before use.
#[derive(Clone, Copy)]
pub enum DefinitionDescription<'a> {
    Path(&'a DefinitionPath),
    Scoped(DefinitionView<'a>),
}

impl<'a> DefinitionDescription<'a> {
    pub fn module(self) -> &'a ModuleIdentity {
        match self {
            Self::Path(path) => &path.module,
            Self::Scoped(view) => view.module(),
        }
    }

    pub fn segments(self) -> impl Iterator<Item = DefinitionSegment<'a>> {
        let (path, scoped) = match self {
            Self::Path(path) => (Some(path.path.iter()), None),
            Self::Scoped(view) => (None, Some(view.segments())),
        };
        path.into_iter()
            .flatten()
            .map(|part| DefinitionSegment {
                kind: part.kind,
                name: &part.name,
                occurrence: part.occurrence,
            })
            .chain(scoped.into_iter().flatten())
    }

    pub fn last(self) -> Option<DefinitionSegment<'a>> {
        self.segments().last()
    }

    pub fn nominal(self, kind: DefinitionKind) -> bool {
        !self.module().package.0.is_empty()
            && !self.module().path.is_empty()
            && !self.module().path.iter().any(String::is_empty)
            && self
                .last()
                .is_some_and(|part| part.kind == kind && !part.name.is_empty())
    }

    pub fn associated_member(self, parent: Self) -> bool {
        if self.module() != parent.module() {
            return false;
        }
        let mut members = self.segments();
        for segment in parent.segments() {
            if members.next().is_none_or(|part| {
                part.kind != segment.kind
                    || part.name != segment.name
                    || part.occurrence != segment.occurrence
            }) {
                return false;
            }
        }
        members
            .next()
            .is_some_and(|part| part.kind == DefinitionKind::AssociatedType && part.occurrence == 0)
            && members.next().is_none()
    }
}
