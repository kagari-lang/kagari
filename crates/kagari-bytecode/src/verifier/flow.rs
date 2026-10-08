//! Independent bounded flow checks for executable resume bodies.
use crate::{
    instruction::BytecodeInstruction as I,
    module::BytecodeFunction,
    suspension::{AwaitLiveness, FlowBudget},
    verifier::BytecodeVerificationError as Error,
};
use kagari_abi::representation::ValueType;
use kagari_types::ty::Ty;
use std::{collections::BTreeSet, mem};

const MAX_FLOW_BYTES: usize = 64 * 1024 * 1024;
const MAX_FLOW_WORK: usize = 100_000_000;

struct Block {
    start: usize,
    end: usize,
    successors: Vec<usize>,
    predecessors: Vec<usize>,
    depth: Option<usize>,
    uses: Vec<u64>,
    defines: Vec<u64>,
    initialized: Vec<u64>,
    live: Vec<u64>,
}

struct Flow<'a> {
    function: &'a BytecodeFunction,
    budget: &'a mut FlowBudget,
    blocks: Vec<Block>,
    registers: usize,
    words: usize,
}

pub(super) fn verify(
    function: &BytecodeFunction,
    budget: &mut FlowBudget,
) -> Result<Vec<AwaitLiveness>, Error> {
    if !function.metadata.effects.may_suspend {
        return Ok(vec![]);
    }
    let registers = usize::from(function.register_count);
    let mut flow = Flow {
        function,
        budget,
        blocks: vec![],
        registers,
        words: (registers + usize::from(function.local_count)).div_ceil(64),
    };
    flow.graph()?;
    flow.resources()?;
    flow.fixed_points()?;
    flow.check_points()
}

impl Flow<'_> {
    fn invalid(&self, reason: &'static str) -> Error {
        Error::InvalidOperation {
            function: self.function.id,
            reason,
        }
    }

    fn charge(&mut self, count: usize) -> Result<(), Error> {
        self.budget.work = self.budget.work.saturating_add(count);
        if self.budget.work > MAX_FLOW_WORK {
            return Err(self.invalid("async flow analysis work limit"));
        }
        Ok(())
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
        let bytes = leaders.len().saturating_mul(
            self.words
                .saturating_mul(32)
                .saturating_add(mem::size_of::<Block>() + 64),
        );
        if bytes > MAX_FLOW_BYTES {
            return Err(self.invalid("async flow analysis state limit"));
        }
        let leaders: Vec<_> = leaders.into_iter().collect();
        for (index, &start) in leaders.iter().enumerate() {
            let end = leaders.get(index + 1).copied().unwrap_or(code.len());
            let mut block = Block {
                start,
                end,
                successors: vec![],
                predecessors: vec![],
                depth: None,
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
            .register_inputs()
            .into_iter()
            .map(|r| r.index())
            .collect::<Vec<_>>();
        if let I::LoadLocal { local, .. } = instruction {
            inputs.push(self.registers + local.index());
        }
        self.charge(self.function.metadata.debug.local_live_ranges.len())?;
        inputs.extend(
            self.function
                .metadata
                .debug
                .local_live_ranges
                .iter()
                .filter(|range| range.start <= pc && pc < range.end)
                .map(|range| self.registers + range.local.index()),
        );
        Ok(inputs)
    }

    fn output(&self, instruction: &I) -> Option<usize> {
        if let I::StoreLocal { local, .. } = instruction {
            return Some(self.registers + local.index());
        }
        instruction.register_output().map(|r| r.index())
    }

    fn resources(&mut self) -> Result<(), Error> {
        self.blocks[0].depth = Some(0);
        let mut pending = vec![0];
        while let Some(index) = pending.pop() {
            let mut depth = self.blocks[index].depth.expect("reachable block");
            for pc in self.blocks[index].start..self.blocks[index].end {
                self.charge(1)?;
                match &self.function.instructions[pc] {
                    I::BeginIteration { .. } => depth += 1,
                    I::EndIteration => {
                        depth = depth
                            .checked_sub(1)
                            .ok_or_else(|| self.invalid("iteration resource underflow"))?
                    }
                    _ => {}
                }
            }
            for successor in self.blocks[index].successors.clone() {
                match self.blocks[successor].depth {
                    Some(previous) if previous != depth => {
                        return Err(self.invalid("inconsistent iteration resource stack"));
                    }
                    Some(_) => {}
                    None => {
                        self.blocks[successor].depth = Some(depth);
                        pending.push(successor);
                    }
                }
            }
        }
        Ok(())
    }

    fn incoming(&self, index: usize) -> Vec<u64> {
        if index == 0 {
            let mut state = vec![0; self.words];
            for parameter in 0..usize::from(self.function.parameter_count) {
                insert(&mut state, self.registers + parameter);
            }
            state
        } else {
            let mut state = vec![u64::MAX; self.words];
            for &previous in &self.blocks[index].predecessors {
                if self.blocks[previous].depth.is_some() {
                    for (word, &initialized) in
                        state.iter_mut().zip(&self.blocks[previous].initialized)
                    {
                        *word &= initialized;
                    }
                }
            }
            state
        }
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

    fn fixed_points(&mut self) -> Result<(), Error> {
        loop {
            let mut changed = false;
            for index in 0..self.blocks.len() {
                if self.blocks[index].depth.is_none() {
                    continue;
                }
                self.charge(
                    self.words.saturating_mul(
                        self.blocks[index].predecessors.len()
                            + self.blocks[index].successors.len()
                            + 4,
                    ) + 1,
                )?;
                let mut initialized = self.incoming(index);
                let mut live = self.live_out(index);
                let block = &mut self.blocks[index];
                for word in 0..self.words {
                    initialized[word] |= block.defines[word];
                    changed |= block.initialized[word] != initialized[word];
                    block.initialized[word] = initialized[word];
                    live[word] = (live[word] & !block.defines[word]) | block.uses[word];
                }
                changed |= block.live != live;
                block.live = live;
            }
            if !changed {
                return Ok(());
            }
        }
    }

    fn check_points(&mut self) -> Result<Vec<AwaitLiveness>, Error> {
        let mut points = Vec::new();
        for index in 0..self.blocks.len() {
            if self.blocks[index].depth.is_none() {
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
                    return Err(self.invalid("uninitialized slot in resume body"));
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
                    self.charge(self.registers + usize::from(self.function.local_count))?;
                    for (slot, &physical) in self
                        .function
                        .metadata
                        .registers
                        .iter()
                        .chain(&self.function.metadata.locals)
                        .enumerate()
                    {
                        let semantic = if slot < self.registers {
                            self.function.metadata.semantic.registers.get(&slot)
                        } else {
                            self.function
                                .metadata
                                .semantic
                                .locals
                                .get(&(slot - self.registers))
                        };
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

fn inline_host(ty: &Ty) -> bool {
    match ty {
        Ty::Host(_) => true,
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
