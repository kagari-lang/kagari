//! Bounded initialization and suspension flow over executable control flow.
use crate::{
    availability::DebugAvailability,
    instruction::BytecodeInstruction as I,
    module::BytecodeFunction,
    suspension::{AwaitLiveness, FlowBudget},
    verifier::BytecodeVerificationError as Error,
};
use kagari_abi::representation::ValueType;
use kagari_types::ty::Ty;
use std::{collections::BTreeSet, mem};

mod solver;

const MAX_FLOW_BYTES: usize = 64 * 1024 * 1024;
const MAX_FLOW_WORK: usize = 100_000_000;

pub(crate) struct Block {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) successors: Vec<usize>,
    pub(crate) predecessors: Vec<usize>,
    pub(crate) reachable: bool,
    uses: Vec<u64>,
    defines: Vec<u64>,
    initialized: Vec<u64>,
    live: Vec<u64>,
}

struct Flow<'a> {
    function: &'a BytecodeFunction,
    budget: &'a mut FlowBudget,
    availability: DebugAvailability<'a>,
    blocks: Vec<Block>,
    registers: usize,
    words: usize,
    #[cfg(test)]
    visits: [usize; 2],
}

pub(super) fn verify(
    function: &BytecodeFunction,
    budget: &mut FlowBudget,
) -> Result<FlowResult, Error> {
    if function.metadata.effects.may_suspend
        && function
            .metadata
            .semantic
            .params
            .values()
            .any(Ty::contains_stored_reference)
    {
        return Err(Error::InvalidOperation {
            function: function.id,
            reason: "a scoped reference cannot be retained in a resume body",
        });
    }
    let registers = function.metadata.slots.len();
    if DebugAvailability::scratch_bytes(function.metadata.debug.local_live_ranges.len())
        > MAX_FLOW_BYTES
    {
        return Err(Error::InvalidOperation {
            function: function.id,
            reason: "executable flow analysis state limit",
        });
    }
    let availability =
        DebugAvailability::new(&function.metadata.debug.local_live_ranges, |count| {
            charge(function, budget, count)
        })?;
    let mut flow = Flow {
        function,
        budget,
        availability,
        blocks: vec![],
        registers,
        words: registers.div_ceil(64),
        #[cfg(test)]
        visits: [0; 2],
    };
    flow.graph()?;
    flow.reachability()?;
    flow.fixed_points()?;
    let awaits = flow.check_points()?;
    Ok(FlowResult {
        awaits,
        blocks: flow.blocks,
    })
}

impl Flow<'_> {
    fn invalid(&self, reason: &'static str) -> Error {
        Error::InvalidOperation {
            function: self.function.id,
            reason,
        }
    }

    fn charge(&mut self, count: usize) -> Result<(), Error> {
        charge(self.function, self.budget, count)
    }

    fn graph(&mut self) -> Result<(), Error> {
        let code = &self.function.instructions;
        let mut leaders = BTreeSet::from([0]);
        for (pc, instruction) in code.iter().enumerate() {
            self.charge(1)?;
            match instruction {
                I::Jump { target } => {
                    leaders.insert(target.index());
                }
                I::Branch {
                    then_target,
                    else_target,
                    ..
                }
                | I::PredicateBranch {
                    then_target,
                    else_target,
                    ..
                } => {
                    leaders.insert(then_target.index());
                    leaders.insert(else_target.index());
                }
                I::Return(_) | I::Unreachable => {}
                _ => continue,
            }
            if pc + 1 < code.len() {
                leaders.insert(pc + 1);
            }
        }
        let bytes = state_bytes(
            leaders.len(),
            self.words,
            self.function.metadata.debug.local_live_ranges.len(),
        );
        if bytes > MAX_FLOW_BYTES {
            return Err(self.invalid("executable flow analysis state limit"));
        }
        let leaders: Vec<_> = leaders.into_iter().collect();
        for (index, &start) in leaders.iter().enumerate() {
            let end = leaders.get(index + 1).copied().unwrap_or(code.len());
            let mut block = Block {
                start,
                end,
                successors: vec![],
                predecessors: vec![],
                reachable: false,
                uses: vec![0; self.words],
                defines: vec![0; self.words],
                initialized: vec![u64::MAX; self.words],
                live: vec![0; self.words],
            };
            for (pc, instruction) in code.iter().enumerate().take(end).skip(start) {
                let inputs = self.inputs(pc)?;
                self.charge(inputs.len() + 1)?;
                for input in inputs {
                    if !contains(&block.defines, input) {
                        insert(&mut block.uses, input);
                    }
                }
                if let Some(output) = self.output(instruction) {
                    insert(&mut block.defines, output);
                }
            }
            let mut successor = |pc| {
                block
                    .successors
                    .push(leaders.binary_search(&pc).expect("checked branch leader"))
            };
            match &code[end - 1] {
                I::Jump { target } => successor(target.index()),
                I::Branch {
                    then_target,
                    else_target,
                    ..
                }
                | I::PredicateBranch {
                    then_target,
                    else_target,
                    ..
                } => {
                    successor(then_target.index());
                    successor(else_target.index());
                }
                I::Return(_) | I::Unreachable => {}
                _ => successor(end),
            }
            self.blocks.push(block);
        }
        for index in 0..self.blocks.len() {
            for successor in self.blocks[index].successors.clone() {
                self.blocks[successor].predecessors.push(index);
            }
        }
        Ok(())
    }

    fn inputs(&mut self, pc: usize) -> Result<Vec<usize>, Error> {
        let instruction = &self.function.instructions[pc];
        let mut inputs = instruction
            .slot_inputs()
            .into_iter()
            .map(|r| r.index())
            .collect::<Vec<_>>();
        self.availability.visit(
            pc,
            |count| charge(self.function, self.budget, count),
            |slot| inputs.push(slot.index()),
        )?;
        Ok(inputs)
    }

    fn output(&self, instruction: &I) -> Option<usize> {
        instruction.slot_output().map(|r| r.index())
    }

    fn reachability(&mut self) -> Result<(), Error> {
        self.blocks[0].reachable = true;
        let mut pending = vec![0];
        while let Some(index) = pending.pop() {
            for successor in self.blocks[index].successors.clone() {
                self.charge(1)?;
                if !self.blocks[successor].reachable {
                    self.blocks[successor].reachable = true;
                    pending.push(successor);
                }
            }
        }
        Ok(())
    }

    fn incoming(&self, index: usize) -> Vec<u64> {
        let mut state = if index == 0 {
            let mut state = vec![0; self.words];
            for parameter in &self.function.metadata.parameter_slots {
                insert(&mut state, parameter.index());
            }
            state
        } else {
            vec![u64::MAX; self.words]
        };
        for &previous in &self.blocks[index].predecessors {
            if self.blocks[previous].reachable {
                for (word, &initialized) in state.iter_mut().zip(&self.blocks[previous].initialized)
                {
                    *word &= initialized;
                }
            }
        }
        state
    }

    fn live_out(&self, index: usize) -> Vec<u64> {
        let mut state = vec![0; self.words];
        for &successor in &self.blocks[index].successors {
            for (word, &live) in state.iter_mut().zip(&self.blocks[successor].live) {
                *word |= live;
            }
        }
        state
    }

    fn check_points(&mut self) -> Result<Vec<AwaitLiveness>, Error> {
        let mut points = Vec::new();
        for index in 0..self.blocks.len() {
            if !self.blocks[index].reachable {
                continue;
            }
            let (start, end) = (self.blocks[index].start, self.blocks[index].end);
            let mut initialized = self.incoming(index);
            let mut live = self.live_out(index);
            for pc in start..end {
                let instruction = &self.function.instructions[pc];
                let inputs = self.inputs(pc)?;
                self.charge(inputs.len() + 1)?;
                if inputs.iter().any(|&input| !contains(&initialized, input)) {
                    return Err(self.invalid("uninitialized executable slot"));
                }
                if let Some(output) = self.output(instruction) {
                    insert(&mut initialized, output);
                }
            }
            for pc in (start..end).rev() {
                let instruction = &self.function.instructions[pc];
                if let Some(output) = self.output(instruction) {
                    live[output / 64] &= !(1 << (output % 64));
                }
                let inputs = self.inputs(pc)?;
                self.charge(inputs.len() + 1)?;
                for input in inputs {
                    insert(&mut live, input);
                }
                if matches!(instruction, I::Await { .. }) {
                    self.charge(self.registers)?;
                    for (slot, &physical) in self.function.metadata.slots.iter().enumerate() {
                        let semantic = self.function.metadata.semantic.slots.get(&slot);
                        if contains(&live, slot)
                            && (physical == ValueType::HostHandle
                                || semantic.is_some_and(inline_host))
                        {
                            return Err(self.invalid("host capability live across await"));
                        }
                    }
                    self.budget.retained_bytes = self
                        .budget
                        .retained_bytes
                        .saturating_add(self.words * 8 + mem::size_of::<AwaitLiveness>());
                    if self.budget.retained_bytes > MAX_FLOW_BYTES {
                        return Err(self.invalid("async suspension facts size limit"));
                    }
                    points.push(AwaitLiveness {
                        instruction: pc,
                        live: live.clone().into_boxed_slice(),
                    });
                }
            }
        }
        points.sort_unstable_by_key(AwaitLiveness::instruction);
        Ok(points)
    }
}

fn state_bytes(blocks: usize, words: usize, ranges: usize) -> usize {
    // Existing edge/leader allowance plus the peak DFS scratch: one reserved
    // stack of (block, edge), postorder and visited flags (under 32 bytes/block).
    // Worklist scratch reuses the postorder and has a smaller peak.
    blocks
        .saturating_mul(
            words
                .saturating_mul(32)
                .saturating_add(mem::size_of::<Block>() + 64 + 32),
        )
        .saturating_add(words.saturating_mul(16))
        .saturating_add(DebugAvailability::scratch_bytes(ranges))
}

fn charge(function: &BytecodeFunction, budget: &mut FlowBudget, count: usize) -> Result<(), Error> {
    budget.cancel.check().map_err(|_| Error::Cancelled)?;
    budget.work = budget.work.saturating_add(count);
    if budget.work > MAX_FLOW_WORK {
        return Err(Error::InvalidOperation {
            function: function.id,
            reason: "executable flow analysis work limit",
        });
    }
    Ok(())
}

pub(super) fn inline_host(ty: &Ty) -> bool {
    match ty {
        Ty::Host(_) | Ty::Reference { .. } => true,
        Ty::Tuple(items) => items.iter().any(inline_host),
        _ => false,
    }
}

fn contains(words: &[u64], slot: usize) -> bool {
    words[slot / 64] & (1 << (slot % 64)) != 0
}

fn insert(words: &mut [u64], slot: usize) {
    words[slot / 64] |= 1 << (slot % 64);
}

/// Shared, independently checked CFG; access analysis consumes these same edges.
pub(crate) struct FlowResult {
    pub(crate) awaits: Vec<AwaitLiveness>,
    pub(crate) blocks: Vec<Block>,
}
