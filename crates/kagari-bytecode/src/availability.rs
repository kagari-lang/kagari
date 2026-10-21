//! Function-local interval queries over admitted debug binding ranges.
use crate::{instruction::Slot, module::LocalLiveRange};
use std::mem;

/// Borrowed, immutable availability index for one exact function's debug records.
///
/// Construction and queries charge caller-owned work; callers must admit
/// [`Self::scratch_bytes`] before construction. Range bounds and slots must already
/// be validated by the bytecode owner. This index establishes no verification seal.
/// Membership is half-open (`start <= pc < end`), including repeated slots once
/// per covering record. Queries work in arbitrary PC order without cursor resets.
/// Dropping the index releases its scratch; editing the borrowed records is forbidden.
#[derive(Debug)]
pub struct DebugAvailability<'a> {
    nodes: Vec<Node<'a>>,
}

#[derive(Debug)]
struct Node<'a> {
    range: &'a LocalLiveRange,
    ordinal: usize,
    max_end: usize,
}

impl<'a> DebugAvailability<'a> {
    /// Upper bound on heap scratch, including empty ranges, with saturating overflow.
    pub fn scratch_bytes(ranges: usize) -> usize {
        ranges.saturating_mul(mem::size_of::<Node<'_>>())
    }

    /// Build a balanced start-ordered interval index with fallible work accounting.
    ///
    /// Sorting costs O(D log D), indexing O(D), and scratch O(D). The callback may
    /// reject work or cancellation at each bounded sorting/traversal step; its error
    /// is returned without publishing a partial index. No source records are reordered.
    pub fn new<E>(
        ranges: &'a [LocalLiveRange],
        mut charge: impl FnMut(usize) -> Result<(), E>,
    ) -> Result<Self, E> {
        charge(ranges.len())?;
        let mut nodes = Vec::with_capacity(ranges.len());
        for (ordinal, range) in ranges.iter().enumerate() {
            charge(1)?;
            if range.start < range.end {
                nodes.push(Node {
                    range,
                    ordinal,
                    max_end: range.end,
                });
            }
        }
        // Fallible heapsort keeps cancellation inside sorting, not just around a
        // potentially large standard-library sort. Its keys never invoke user code.
        for root in (0..nodes.len() / 2).rev() {
            sift(&mut nodes, root, &mut charge)?;
        }
        for end in (1..nodes.len()).rev() {
            charge(1)?;
            nodes.swap(0, end);
            sift(&mut nodes[..end], 0, &mut charge)?;
        }
        index(&mut nodes, &mut charge)?;
        Ok(Self { nodes })
    }

    /// Visit covering slots in deterministic (range start, original ordinal) order.
    ///
    /// Max-end pruning avoids full-table filtering. Worst-case work is O(D), with
    /// sparse queries pruned along balanced paths; dense overlap must enumerate its
    /// outputs. Recursion depth is O(log D), bounded by the machine index width.
    /// Callback errors stop the query immediately. An empty index charges no work.
    pub fn visit<E>(
        &self,
        pc: usize,
        mut charge: impl FnMut(usize) -> Result<(), E>,
        mut slot: impl FnMut(Slot),
    ) -> Result<(), E> {
        visit(&self.nodes, pc, &mut charge, &mut slot)
    }
}

fn key(node: &Node<'_>) -> (usize, usize) {
    (node.range.start, node.ordinal)
}

fn sift<E>(
    nodes: &mut [Node<'_>],
    mut root: usize,
    charge: &mut impl FnMut(usize) -> Result<(), E>,
) -> Result<(), E> {
    while root < nodes.len() / 2 {
        charge(3)?;
        let mut child = root * 2 + 1;
        if child + 1 < nodes.len() && key(&nodes[child]) < key(&nodes[child + 1]) {
            child += 1;
        }
        if key(&nodes[root]) >= key(&nodes[child]) {
            break;
        }
        nodes.swap(root, child);
        root = child;
    }
    Ok(())
}

fn index<E>(
    nodes: &mut [Node<'_>],
    charge: &mut impl FnMut(usize) -> Result<(), E>,
) -> Result<usize, E> {
    if nodes.is_empty() {
        return Ok(0);
    }
    charge(1)?;
    let mid = nodes.len() / 2;
    let (left, rest) = nodes.split_at_mut(mid);
    let (node, right) = rest.split_first_mut().expect("nonempty interval subtree");
    node.max_end = node
        .range
        .end
        .max(index(left, charge)?)
        .max(index(right, charge)?);
    Ok(node.max_end)
}

fn visit<E>(
    nodes: &[Node<'_>],
    pc: usize,
    charge: &mut impl FnMut(usize) -> Result<(), E>,
    slot: &mut impl FnMut(Slot),
) -> Result<(), E> {
    if nodes.is_empty() {
        return Ok(());
    }
    charge(1)?;
    let mid = nodes.len() / 2;
    let node = &nodes[mid];
    if node.max_end <= pc {
        return Ok(());
    }
    visit(&nodes[..mid], pc, charge, slot)?;
    if node.range.start <= pc {
        if pc < node.range.end {
            charge(1)?;
            slot(node.range.slot);
        }
        visit(&nodes[mid + 1..], pc, charge, slot)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
