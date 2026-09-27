use std::collections::VecDeque;
use std::mem;

use kagari_abi::budget::LogicalBudgetCharge;
use kagari_abi::representation::ValueType;

use crate::analysis::{BlockAnalysis, FunctionAnalysis, PointAnalysis, SafepointKind, SlotSet};
use crate::verify::flow::{self, Initialization};
use crate::verify::{Context, MirVerificationError};
use crate::{Instruction, MirFunction};

const MAX_ANALYSIS_BYTES: usize = 64 * 1024 * 1024;
const MAX_ANALYSIS_WORK: usize = 100_000_000;

/// One budget for the entire verification request, including all program modules
/// and their forward and backward fixed points.
/// Account for retained facts and scratch matrices before allocating them.
#[derive(Default)]
pub(super) struct Budget {
    bytes: usize,
    work: usize,
}

impl Budget {
    pub(super) fn work(
        &mut self,
        count: usize,
        context: Context<'_>,
    ) -> Result<(), MirVerificationError> {
        context.check_cancel()?;
        self.work = self.work.saturating_add(count);
        context.limit(self.work, MAX_ANALYSIS_WORK, "MIR analysis work")
    }
}

pub(super) fn reserve(
    function: &MirFunction,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<(), MirVerificationError> {
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let points = function.blocks.iter().fold(0usize, |count, block| {
        count
            .saturating_add(block.instructions.len())
            .saturating_add(1)
    });
    let point_bytes = mem::size_of::<PointAnalysis>().saturating_add(words.saturating_mul(24));
    let block_bytes = mem::size_of::<BlockAnalysis>()
        .saturating_add(words.saturating_mul(24))
        .saturating_add(256); // queues, reachability, predecessor capacity and vector headers
    budget.bytes = budget
        .bytes
        .saturating_add(mem::size_of::<FunctionAnalysis>().saturating_mul(2))
        .saturating_add(points.saturating_mul(point_bytes))
        .saturating_add(function.blocks.len().saturating_mul(block_bytes))
        .saturating_add(words.saturating_mul(48)); // function-wide scratch bitsets
    context.limit(budget.bytes, MAX_ANALYSIS_BYTES, "MIR analysis state bytes")
}

pub(super) fn build(
    function: &MirFunction,
    initialized: Initialization,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<FunctionAnalysis, MirVerificationError> {
    let mut blocks = debug_points(function, &initialized, context, budget)?;
    let mut logical_offset = 0;
    for (index, _) in function.emission_order() {
        for point in &mut blocks[index].points {
            budget.work(1, context)?;
            point.logical_offset = logical_offset;
            logical_offset += 1;
        }
    }
    let entries = live_entries(function, &initialized, &blocks, context, budget)?;
    record_liveness(function, &mut blocks, &entries, context, budget)?;
    Ok(FunctionAnalysis { blocks })
}

fn debug_points(
    function: &MirFunction,
    initialized: &Initialization,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<Vec<BlockAnalysis>, MirVerificationError> {
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let empty = SlotSet {
        words: vec![0; words],
        temps: function.temps.len(),
        locals: function.locals.len(),
    };
    let mut blocks = Vec::with_capacity(function.blocks.len());
    for (index, block) in function.blocks.iter().enumerate() {
        let mut state = initialized.entries[index].clone();
        let mut points = Vec::with_capacity(block.instructions.len() + 1);
        for (offset, &scope) in block
            .instruction_scopes
            .iter()
            .chain(block.terminator_scope.iter())
            .enumerate()
        {
            budget.work(words + 1, context)?;
            let mut debug_available = empty.clone();
            if initialized.reachable[index] {
                budget.work(function.params.len(), context)?;
                for param in &function.params {
                    flow::insert(
                        &mut debug_available.words,
                        function.temps.len() + param.local.index(),
                    );
                }
                let mut cursor = Some(scope);
                while let Some(index) = cursor {
                    budget.work(1, context)?;
                    let scope = &function.debug.lexical_scopes[index];
                    if let Some(local) = scope.local {
                        let slot = function.temps.len() + local.index();
                        if flow::contains(&state, slot) {
                            flow::insert(&mut debug_available.words, slot);
                        }
                    }
                    cursor = scope.parent;
                }
            }
            let safepoint =
                block
                    .instructions
                    .get(offset)
                    .map_or(SafepointKind::ControlFlow, |instruction| {
                        let effects = instruction.effects();
                        if effects.calls || effects.allocates || effects.touches_runtime {
                            SafepointKind::Runtime
                        } else {
                            SafepointKind::Budget
                        }
                    });
            points.push(PointAnalysis {
                logical_offset: 0,
                budget: LogicalBudgetCharge::Step,
                live: empty.clone(),
                roots: empty.clone(),
                debug_available,
                safepoint,
            });
            if let Some(instruction) = block.instructions.get(offset) {
                flow::define(function, instruction, &mut state);
            }
        }
        blocks.push(BlockAnalysis {
            reachable: initialized.reachable[index],
            points,
        });
    }
    Ok(blocks)
}

fn live_entries(
    function: &MirFunction,
    initialized: &Initialization,
    blocks: &[BlockAnalysis],
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<Vec<Vec<u64>>, MirVerificationError> {
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let mut entries = vec![vec![0; words]; function.blocks.len()];
    let mut queue: VecDeque<_> = (0..blocks.len())
        .filter(|&index| initialized.reachable[index])
        .collect();
    let mut queued = initialized.reachable.clone();
    while let Some(index) = queue.pop_front() {
        queued[index] = false;
        let mut state =
            live_at_terminator(function, index, &blocks[index], &entries, context, budget)?;
        for (instruction, point) in function.blocks[index]
            .instructions
            .iter()
            .zip(&blocks[index].points)
            .rev()
        {
            transfer(function, instruction, point, &mut state, context, budget)?;
        }
        if state != entries[index] {
            entries[index] = state;
            for &predecessor in &initialized.predecessors[index] {
                if !mem::replace(&mut queued[predecessor], true) {
                    queue.push_back(predecessor);
                }
            }
        }
    }
    Ok(entries)
}

fn record_liveness(
    function: &MirFunction,
    blocks: &mut [BlockAnalysis],
    entries: &[Vec<u64>],
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<(), MirVerificationError> {
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let mut heap = vec![0; words];
    for (index, temp) in function.temps.iter().enumerate() {
        if temp.ty == ValueType::HeapObject {
            flow::insert(&mut heap, index);
        }
    }
    for (index, local) in function.locals.iter().enumerate() {
        if local.ty == ValueType::HeapObject {
            flow::insert(&mut heap, function.temps.len() + index);
        }
    }
    for (index, block) in blocks.iter_mut().enumerate() {
        if !block.reachable {
            continue;
        }
        let mut state = live_at_terminator(function, index, block, entries, context, budget)?;
        save(block.points.last_mut().expect("terminator"), &state, &heap);
        for (instruction, point) in function.blocks[index]
            .instructions
            .iter()
            .zip(&mut block.points)
            .rev()
        {
            transfer(function, instruction, point, &mut state, context, budget)?;
            save(point, &state, &heap);
        }
    }
    Ok(())
}

fn live_at_terminator(
    function: &MirFunction,
    index: usize,
    block: &BlockAnalysis,
    entries: &[Vec<u64>],
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<Vec<u64>, MirVerificationError> {
    let mut state = block.terminator().debug_available.words.clone();
    let terminator = function.blocks[index]
        .terminator
        .as_ref()
        .expect("verified terminator");
    budget.work(state.len().saturating_mul(3).saturating_add(1), context)?;
    for successor in terminator.successors() {
        for (word, &incoming) in state.iter_mut().zip(&entries[successor.index()]) {
            *word |= incoming;
        }
    }
    if let Some(value) = terminator.input() {
        flow::insert(&mut state, value.temp.index());
    }
    Ok(state)
}

fn transfer(
    function: &MirFunction,
    instruction: &Instruction,
    point: &PointAnalysis,
    state: &mut [u64],
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<(), MirVerificationError> {
    let inputs = instruction.inputs();
    budget.work(
        state.len().saturating_add(inputs.len()).saturating_add(1),
        context,
    )?;
    if let Some(output) = instruction.output() {
        remove(state, output.temp.index());
    }
    if let Instruction::StoreLocal { local, .. } = instruction {
        remove(state, function.temps.len() + local.index());
    }
    for value in inputs {
        flow::insert(state, value.temp.index());
    }
    if let Instruction::LoadLocal { local, .. } = instruction {
        flow::insert(state, function.temps.len() + local.index());
    }
    for (word, &visible) in state.iter_mut().zip(&point.debug_available.words) {
        *word |= visible;
    }
    Ok(())
}

fn remove(state: &mut [u64], index: usize) {
    state[index / 64] &= !(1 << (index % 64));
}

fn save(point: &mut PointAnalysis, state: &[u64], heap: &[u64]) {
    point.live.words.copy_from_slice(state);
    for ((root, &live), &heap) in point.roots.words.iter_mut().zip(state).zip(heap) {
        *root = live & heap;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MirVerificationErrorKind;
    use kagari_common::cancellation::CancellationToken;

    #[test]
    fn work_is_bounded_across_fixed_points_and_observes_cancellation() {
        let cancel = CancellationToken::default();
        let context = Context {
            function: None,
            block: None,
            instruction: None,
            span: None,
            cancel: &cancel,
        };
        let mut budget = Budget::default();
        budget.work(MAX_ANALYSIS_WORK - 1, context).unwrap();
        budget.work(1, context).unwrap();
        assert_eq!(
            budget.work(1, context).unwrap_err().kind,
            MirVerificationErrorKind::Limit {
                resource: "MIR analysis work",
                limit: MAX_ANALYSIS_WORK,
            }
        );
        cancel.cancel();
        assert_eq!(
            Budget::default().work(0, context).unwrap_err().kind,
            MirVerificationErrorKind::Cancelled
        );
    }
}
