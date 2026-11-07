//! Bounded pure reachability memo for one immutable namespace catalog.

use crate::{imports::SourceDeclRef, resolver::table::NameTable};
use std::{
    collections::HashSet,
    mem::size_of,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

const MAX_SCOPES: usize = 1024;
const MAX_SOURCE_CAPACITY: usize = 65_536;
const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug)]
struct Entry {
    names: Weak<NameTable>,
    sources: Arc<HashSet<SourceDeclRef>>,
}

#[derive(Debug, Default)]
struct State {
    entries: Vec<Entry>,
}

/// Only exact name-table allocations in this catalog can share a complete set.
/// The fixed source-reference fields own no strings, vectors or source allocations.
#[derive(Debug, Default)]
pub(super) struct ReachabilityCache {
    state: Mutex<State>,
}

impl Clone for ReachabilityCache {
    fn clone(&self) -> Self {
        // A catalog clone may be changed before publication. Its scoped references
        // must never inherit a projection computed from the original catalog.
        Self::default()
    }
}

impl ReachabilityCache {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    pub(super) fn get(&self, names: &Arc<NameTable>) -> Option<Arc<HashSet<SourceDeclRef>>> {
        self.lock()
            .entries
            .iter()
            .find(|entry| entry.names.as_ptr() == Arc::as_ptr(names))
            .map(|entry| entry.sources.clone())
    }

    /// Budget exhaustion changes reuse only; the complete computed result survives.
    pub(super) fn insert(
        &self,
        names: &Arc<NameTable>,
        sources: Arc<HashSet<SourceDeclRef>>,
    ) -> Arc<HashSet<SourceDeclRef>> {
        let mut state = self.lock();
        if let Some(entry) = state
            .entries
            .iter()
            .find(|entry| entry.names.as_ptr() == Arc::as_ptr(names))
        {
            return entry.sources.clone();
        }
        state.entries.retain(|entry| entry.names.strong_count() > 0);
        let capacity = state.entries.iter().fold(sources.capacity(), |sum, entry| {
            sum.saturating_add(entry.sources.capacity())
        });
        let count = state.entries.len().saturating_add(1);
        // Hash-table capacity excludes control bytes and spare buckets. Twice the
        // fixed slot size plus per-entry/header allowances conservatively charges
        // requested storage, rather than equating initialized length with memory.
        // Allocator/RSS overhead remains the benchmark's separate responsibility.
        let bytes = capacity
            .saturating_mul(2 * (size_of::<SourceDeclRef>() + 1))
            .saturating_add(count.saturating_mul(size_of::<NameTable>() + 512))
            .saturating_add(MAX_SCOPES * size_of::<Entry>());
        if count > MAX_SCOPES || capacity > MAX_SOURCE_CAPACITY || bytes > MAX_BYTES {
            return sources;
        }
        if state.entries.try_reserve_exact(1).is_err() {
            return sources;
        }
        state.entries.push(Entry {
            names: Arc::downgrade(names),
            sources: sources.clone(),
        });
        sources
    }

    pub(super) fn clear(&self) {
        *self.lock() = State::default();
    }
}

#[cfg(test)]
mod tests;
