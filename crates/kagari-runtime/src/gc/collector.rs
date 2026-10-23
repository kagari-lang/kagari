//! Graph marking independent of script representations and execution semantics.
use std::{collections::HashSet, hash::Hash};

/// Roots and children are appended in stack order. Missing records invalidate the
/// entire mark result, so callers cannot sweep a partially validated graph.
pub(super) fn mark<I: Copy + Eq + Hash>(
    mut pending: Vec<I>,
    mut edges: impl FnMut(I, &mut Vec<I>) -> Option<()>,
) -> Option<Vec<I>> {
    let mut visited = Visited::new();
    let mut live = Vec::new();
    visited.mark(&mut pending, |id, pending| {
        edges(id, pending)?;
        live.push(id);
        Some(())
    })?;
    Some(live)
}

/// One traversal's identity set, shared by sequential root batches only while
/// their owner prevents graph mutation. Value admission remains with the caller.
pub(super) struct Visited<I> {
    seen: HashSet<I>,
}

impl<I: Copy + Eq + Hash> Visited<I> {
    pub(super) fn new() -> Self {
        Self {
            seen: HashSet::new(),
        }
    }

    pub(super) fn mark(
        &mut self,
        pending: &mut Vec<I>,
        mut edges: impl FnMut(I, &mut Vec<I>) -> Option<()>,
    ) -> Option<()> {
        while let Some(id) = pending.pop() {
            if !self.seen.insert(id) {
                continue;
            }
            edges(id, pending)?;
        }
        Some(())
    }
}
