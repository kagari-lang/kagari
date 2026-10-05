//! Graph marking independent of script representations and execution semantics.
use std::{collections::HashSet, hash::Hash};

/// Roots and children are appended in stack order. Missing records invalidate the
/// entire mark result, so callers cannot sweep a partially validated graph.
pub(super) fn mark<I: Copy + Eq + Hash>(
    mut pending: Vec<I>,
    mut edges: impl FnMut(I, &mut Vec<I>) -> Option<()>,
) -> Option<Vec<I>> {
    let mut seen = HashSet::new();
    let mut live = Vec::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        edges(id, &mut pending)?;
        live.push(id);
    }
    Some(live)
}
