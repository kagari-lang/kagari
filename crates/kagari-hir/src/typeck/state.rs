//! Mutable type-resolution workspace, separate from retained semantic facts.

use crate::{hir::ids::TypeRefId, typeck::table::TypeTable, types::TypeId};
use std::{
    collections::{HashMap, HashSet},
    ops::{Deref, DerefMut},
};

/// Owns the facts being built and the temporary state needed to resolve types.
///
/// For `val item: Vec<_> = Vec::new()`, `inference_holes` connects the written
/// `_` to a body-local inference variable. `resolving_types` tracks the active
/// resolution stack, so recursive aliases cannot be mistaken for completed types.
/// Neither collection belongs to a retained signature, query result or identity
/// mapping. The body checker separately owns its solver and traversal stacks.
///
/// Dereferencing borrows only the fact table, allowing read-only semantic helpers
/// and fact writers to share the same operations during checking. Consuming this
/// workspace drops its temporary state; it does not grant compilation eligibility.
#[derive(Clone, Default)]
pub(super) struct TypeCheckState {
    facts: TypeTable,
    pub(super) inference_holes: HashMap<TypeRefId, TypeId>,
    pub(super) resolving_types: HashSet<TypeRefId>,
}

impl TypeCheckState {
    /// Starts a new checking workspace from facts with matching node ownership.
    pub(super) fn from_facts(facts: TypeTable) -> Self {
        Self {
            facts,
            ..Self::default()
        }
    }

    /// Extracts recoverable facts and drops all type-resolution work state.
    ///
    /// Diagnostics and cancellation are checked by the owning analysis stage.
    /// Callers must not interpret this conversion as a checked compilation seal.
    pub(super) fn into_facts(self) -> TypeTable {
        self.facts
    }
}

impl Deref for TypeCheckState {
    type Target = TypeTable;

    fn deref(&self) -> &Self::Target {
        &self.facts
    }
}

impl DerefMut for TypeCheckState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.facts
    }
}
