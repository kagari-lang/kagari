//! Stable, aligned deletion of the grouped decisions produced by `dead::find`.
//!
//! The finder emits contiguous function/block groups with descending instruction
//! indices. Consume that order directly: preparation visits each deletion once,
//! and each affected block is compacted once, without sorting or scratch allocation.
use crate::{
    function::{BasicBlock, MirModule},
    passes::Work,
    verify::MirVerificationError,
};

/// Mutate only unpublished MIR, using the exact grouped output of `dead::find`.
/// Errors may leave raw blocks partially compacted; the caller must discard them.
pub(super) fn apply(
    module: &mut MirModule,
    mut removals: &[(usize, usize, usize)],
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    while let Some(&(function, block, _)) = removals.first() {
        let mut end = 0;
        while end < removals.len() && (removals[end].0, removals[end].1) == (function, block) {
            work.charge(1)?;
            end += 1;
        }
        compact_block(
            &mut module.functions[function].blocks[block],
            removals[..end].iter().rev().map(|&(_, _, index)| index),
            work,
        )?;
        removals = &removals[end..];
    }
    Ok(())
}

/// Indices are valid, unique and ascending, as obtained by reversing one group.
fn compact_block(
    block: &mut BasicBlock,
    removals: impl Iterator<Item = usize>,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    let mut removals = removals.peekable();
    let mut retained = 0;
    for read in 0..block.instructions.len() {
        work.charge(1)?;
        if removals.peek() == Some(&read) {
            removals.next();
            continue;
        }
        if retained != read {
            // Two relocated elements per column; the same keep decision controls
            // all three swaps. Dead entries accumulate in the unpublished tail.
            work.charge(6)?;
            block.instructions.swap(retained, read);
            block.instruction_spans.swap(retained, read);
            block.instruction_scopes.swap(retained, read);
        }
        retained += 1;
    }
    debug_assert!(removals.next().is_none());

    // Instruction destruction also traverses the dead tail. Bound each batch so
    // a large all-dead block cannot defer cancellation until after bulk cleanup.
    while block.instructions.len() > retained {
        let end = block.instructions.len().saturating_sub(256).max(retained);
        work.charge(3 * (block.instructions.len() - end))?;
        block.instructions.truncate(end);
        block.instruction_spans.truncate(end);
        block.instruction_scopes.truncate(end);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
