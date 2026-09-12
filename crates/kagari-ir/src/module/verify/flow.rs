use std::collections::VecDeque;

use super::{
    Context, IrVerificationError, IrVerificationErrorKind as Error, inputs, output, successors,
};
use crate::module::{BlockId, Instruction, IrFunction, Terminator};

// Bound the verifier's quadratic block/value state rather than allocating an
// unbounded matrix for malicious or accidentally enormous IR.
const MAX_FLOW_BYTES: usize = 64 * 1024 * 1024;

pub(super) fn verify(
    function: &IrFunction,
    context: Context<'_>,
) -> Result<(), IrVerificationError> {
    let mut reachable = vec![false; function.blocks.len()];
    let mut pending = vec![function.entry];
    let mut predecessors = vec![Vec::new(); function.blocks.len()];
    while let Some(id) = pending.pop() {
        context.check_cancel()?;
        if std::mem::replace(&mut reachable[id.index()], true) {
            continue;
        }
        for target in successors(
            function.blocks[id.index()]
                .terminator
                .as_ref()
                .expect("checked terminator"),
        ) {
            predecessors[target.index()].push(id.index());
            pending.push(target);
        }
    }
    let words = (function.temps.len() + function.locals.len()).div_ceil(64);
    let bytes = function
        .blocks
        .len()
        .saturating_mul(words)
        .saturating_mul(8);
    context.limit(bytes, MAX_FLOW_BYTES, "definite-initialization state bytes")?;
    let mut outputs = vec![vec![u64::MAX; words]; function.blocks.len()];
    let mut queue: VecDeque<_> = (0..function.blocks.len())
        .filter(|&i| reachable[i])
        .collect();
    let mut queued = reachable.clone();
    while let Some(index) = queue.pop_front() {
        queued[index] = false;
        context.check_cancel()?;
        let mut state = incoming(function, index, words, &predecessors, &outputs);
        for instruction in &function.blocks[index].instructions {
            context.check_cancel()?;
            define(function, instruction, &mut state);
        }
        if state != outputs[index] {
            outputs[index] = state;
            for target in successors(
                function.blocks[index]
                    .terminator
                    .as_ref()
                    .expect("checked terminator"),
            ) {
                if !std::mem::replace(&mut queued[target.index()], true) {
                    queue.push_back(target.index());
                }
            }
        }
    }
    for (index, block) in function.blocks.iter().enumerate() {
        if !reachable[index] {
            continue;
        }
        let context = Context {
            block: Some(BlockId::new(index)),
            ..context
        };
        let mut state = incoming(function, index, words, &predecessors, &outputs);
        for (index, instruction) in block.instructions.iter().enumerate() {
            let context = Context {
                instruction: Some(index),
                span: Some(block.instruction_spans[index]),
                ..context
            };
            context.check_cancel()?;
            for value in inputs(instruction) {
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
        let value = match block.terminator.as_ref().expect("checked terminator") {
            Terminator::Return(value) => *value,
            Terminator::Branch { cond, .. } => Some(*cond),
            _ => None,
        };
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
    Ok(())
}

fn incoming(
    function: &IrFunction,
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

fn define(function: &IrFunction, instruction: &Instruction, state: &mut [u64]) {
    if let Some(dst) = output(instruction) {
        insert(state, dst.temp.index());
    }
    if let Instruction::StoreLocal { local, .. } = instruction {
        insert(state, function.temps.len() + local.index());
    }
}
fn insert(state: &mut [u64], index: usize) {
    state[index / 64] |= 1 << (index % 64);
}
fn contains(state: &[u64], index: usize) -> bool {
    state[index / 64] & (1 << (index % 64)) != 0
}
