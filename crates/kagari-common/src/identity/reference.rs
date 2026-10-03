//! Representations used by the same semantic metadata at explicit boundaries.
use crate::identity::{
    DefinitionPath,
    table::{DefinitionId, wire::PortableDefinitionRef},
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

    /// Only authoring representations contain an owned path. Contextual and
    /// portable references require explicit resolution by their owning table.
    fn authoring_path(&self) -> Option<&DefinitionPath> {
        None
    }
}

impl sealed::Sealed for DefinitionPath {}

impl DefinitionReference for DefinitionPath {
    fn within_path_limit(&self) -> bool {
        self.within_path_limit()
    }

    fn authoring_path(&self) -> Option<&DefinitionPath> {
        Some(self)
    }
}

impl sealed::Sealed for DefinitionId {}

impl DefinitionReference for DefinitionId {
    fn within_path_limit(&self) -> bool {
        true
    }
}

impl sealed::Sealed for PortableDefinitionRef {}

impl DefinitionReference for PortableDefinitionRef {
    fn within_path_limit(&self) -> bool {
        true
    }
}
