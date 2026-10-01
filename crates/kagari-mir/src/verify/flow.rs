use std::{collections::VecDeque, mem};

use crate::{
    function::MirFunction,
    ids::BlockId,
    instruction::Instruction,
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error, analysis::Budget},
};

pub(super) struct Initialization {
    pub reachable: Vec<bool>,
    pub predecessors: Vec<Vec<usize>>,
    pub entries: Vec<Vec<u64>>,
}

// Bound the verifier's quadratic block/value state rather than allocating an
// unbounded matrix for malicious or accidentally enormous IR.
const MAX_FLOW_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn verify(
    function: &MirFunction,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<Initialization, MirVerificationError> {
    let mut reachable = vec![false; function.blocks.len()];
    let mut pending = vec![function.entry];
    let mut predecessors = vec![Vec::new(); function.blocks.len()];
    while let Some(id) = pending.pop() {
        budget.work(1, context)?;
        if mem::replace(&mut reachable[id.index()], true) {
            continue;
        }
        for target in function.blocks[id.index()]
            .terminator
            .as_ref()
            .expect("checked terminator")
            .successors()
        {
            predecessors[target.index()].push(id.index());
            pending.push(target);
        }
    }
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let mut outputs = vec![vec![u64::MAX; words]; function.blocks.len()];
    let mut queue: VecDeque<_> = (0..function.blocks.len())
        .filter(|&i| reachable[i])
        .collect();
    let mut queued = reachable.clone();
    while let Some(index) = queue.pop_front() {
        queued[index] = false;
        context.check_cancel()?;
        budget.work(
            words
                .saturating_mul(predecessors[index].len() + 1)
                .saturating_add(function.params.len())
                .saturating_add(1),
            context,
        )?;
        let mut state = incoming(function, index, words, &predecessors, &outputs);
        for instruction in &function.blocks[index].instructions {
            budget.work(1, context)?;
            define(function, instruction, &mut state);
        }
        if state != outputs[index] {
            outputs[index] = state;
            for target in function.blocks[index]
                .terminator
                .as_ref()
                .expect("checked terminator")
                .successors()
            {
                if !mem::replace(&mut queued[target.index()], true) {
                    queue.push_back(target.index());
                }
            }
        }
    }
    let mut entries = vec![vec![0; words]; function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        let context = Context {
            block: Some(BlockId::new(index)),
            ..context
        };
        budget.work(
            words
                .saturating_mul(predecessors[index].len() + 1)
                .saturating_add(function.params.len())
                .saturating_add(1),
            context,
        )?;
        let mut state = incoming(function, index, words, &predecessors, &outputs);
        entries[index].clone_from(&state);
        for (index, instruction) in block.instructions.iter().enumerate() {
            let context = Context {
                instruction: Some(index),
                span: Some(block.instruction_spans[index]),
                ..context
            };
            let inputs = instruction.inputs();
            budget.work(inputs.len() + 1, context)?;
            for value in inputs {
                if !contains(&state, value.temp.index()) {
                    return Err(context.error(Error::UninitializedTemp(value.temp)));
                }
            }
            if let Instruction::LoadLocal { local, .. } = instruction
                && !contains(&state, function.temps.len() + local.index())
            {
                return Err(context.error(Error::UninitializedLocal(*local)));
            }
            define(function, instruction, &mut state);
        }
        let value = block
            .terminator
            .as_ref()
            .expect("checked terminator")
            .input();
        if let Some(value) = value
            && !contains(&state, value.temp.index())
        {
            return Err(Context {
                span: block.terminator_span,
                ..context
            }
            .error(Error::UninitializedTemp(value.temp)));
        }
    }
    Ok(Initialization {
        reachable,
        predecessors,
        entries,
    })
}

fn incoming(
    function: &MirFunction,
    index: usize,
    words: usize,
    predecessors: &[Vec<usize>],
    outputs: &[Vec<u64>],
) -> Vec<u64> {
    if index == function.entry.index() {
        let mut state = vec![0; words];
        for param in &function.params {
            insert(&mut state, function.temps.len() + param.local.index());
        }
        state
    } else {
        let mut state = vec![u64::MAX; words];
        for &predecessor in &predecessors[index] {
            for (word, out) in state.iter_mut().zip(&outputs[predecessor]) {
                *word &= out;
            }
        }
        state
    }
}

pub(super) fn define(function: &MirFunction, instruction: &Instruction, state: &mut [u64]) {
    if let Some(dst) = instruction.output() {
        insert(state, dst.temp.index());
    }
    if let Instruction::StoreLocal { local, .. } = instruction {
        insert(state, function.temps.len() + local.index());
    }
}
pub(super) fn insert(state: &mut [u64], index: usize) {
    state[index / 64] |= 1 << (index % 64);
}
pub(super) fn contains(state: &[u64], index: usize) -> bool {
    state[index / 64] & (1 << (index % 64)) != 0
}

pub(super) fn check_size(
    function: &MirFunction,
    context: Context<'_>,
) -> Result<(), MirVerificationError> {
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let bytes = function
        .blocks
        .len()
        .saturating_mul(words)
        .saturating_mul(8);
    context.limit(bytes, MAX_FLOW_BYTES, "definite-initialization state bytes")?;
    Ok(())
}
