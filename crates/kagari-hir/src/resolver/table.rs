use std::collections::{HashMap, hash_map::Entry};

use super::ResolvedName;
use crate::hir::ImplId;

/// Presence blocks fallback even when a declaration or import cannot resolve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameResolution {
    Unique(ResolvedName),
    Ambiguous,
    Unresolved,
}

impl NameResolution {
    pub fn target(self) -> Option<ResolvedName> {
        match self {
            Self::Unique(target) => Some(target),
            Self::Ambiguous | Self::Unresolved => None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct NameTable {
    entries: HashMap<String, NameResolution>,
    impls: Vec<ImplId>,
}

impl NameTable {
    /// A collision never chooses a declaration by kind or insertion order.
    pub(crate) fn insert(&mut self, name: String, target: Option<ResolvedName>) -> bool {
        match self.entries.entry(name) {
            Entry::Vacant(entry) => {
                entry.insert(
                    target
                        .map(NameResolution::Unique)
                        .unwrap_or(NameResolution::Unresolved),
                );
                true
            }
            Entry::Occupied(mut entry) => {
                entry.insert(NameResolution::Ambiguous);
                false
            }
        }
    }

    pub fn lookup(&self, name: &str) -> Option<NameResolution> {
        self.entries.get(name).copied()
    }

    pub(crate) fn insert_impl(&mut self, id: ImplId) {
        self.impls.push(id);
    }

    pub fn impl_count(&self) -> usize {
        self.impls.len()
    }
}
