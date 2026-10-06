//! Physical register intervals are derived from verified control flow, not from
//! semantic type inference. Canonical register identities remain unchanged for
//! contracts, native boundaries and debugging. Locals have separate fixed slots.
use crate::module::execution::operands;
use kagari_bytecode::{instruction::BytecodeInstruction, module::BytecodeFunction};
use kagari_common::identity::table::DefinitionId;
use std::collections::BTreeSet;

const MAX_WORK: usize = 4_000_000;
const MAX_WORDS: usize = 1_000_000;

#[derive(Debug)]
pub(crate) struct RegisterAllocation {
    pub slots: Box<[usize]>,
    pub count: usize,
}

#[derive(Default)]
struct Block {
    start: usize,
    end: usize,
    successors: Vec<usize>,
    uses: Vec<u64>,
    defs: Vec<u64>,
    live: Vec<u64>,
    out: Vec<u64>,
}

impl RegisterAllocation {
    pub(super) fn prepare(function: &BytecodeFunction<DefinitionId>, work: &mut usize) -> Self {
        Self::allocate(function, work).unwrap_or_else(|| {
            // Bounded preparation may decline allocation without changing code
            // semantics or accepting unchecked data. This is not an old format.
            let count = usize::from(function.register_count);
            Self {
                slots: (0..count).collect(),
                count,
            }
        })
    }

    pub(crate) fn index(&self, logical: usize) -> Option<usize> {
        self.slots.get(logical).copied()
    }

    fn allocate(function: &BytecodeFunction<DefinitionId>, work: &mut usize) -> Option<Self> {
        let code = &function.instructions;
        let count = usize::from(function.register_count);
        let words = count.div_ceil(64);
        let mut leaders = BTreeSet::from([0]);
        for (pc, instruction) in code.iter().enumerate() {
            charge(work, 1)?;
            match instruction {
                BytecodeInstruction::Jump { target } => {
                    leaders.insert(target.index());
                }
                BytecodeInstruction::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    leaders.insert(then_target.index());
                    leaders.insert(else_target.index());
                }
                BytecodeInstruction::Return(_) | BytecodeInstruction::Unreachable => {}
                _ => continue,
            }
            if pc + 1 < code.len() {
                leaders.insert(pc + 1);
            }
        }
        let leaders: Vec<_> = leaders.into_iter().collect();
        if leaders.len() > 16384
            || words.saturating_mul(leaders.len()).saturating_mul(4) > MAX_WORDS
        {
            return None;
        }
        let mut blocks = Vec::with_capacity(leaders.len());
        let mut ranges = vec![None; count];
        for (index, &start) in leaders.iter().enumerate() {
            let end = leaders.get(index + 1).copied().unwrap_or(code.len());
            let mut block = Block {
                start,
                end,
                uses: vec![0; words],
                defs: vec![0; words],
                live: vec![0; words],
                out: vec![0; words],
                ..Default::default()
            };
            for (pc, instruction) in code.iter().enumerate().take(end).skip(start) {
                for register in operands::reads(instruction) {
                    charge(work, 1)?;
                    let slot = register.index();
                    touch(&mut ranges[slot], pc);
                    let bit = 1 << (slot % 64);
                    if block.defs[slot / 64] & bit == 0 {
                        block.uses[slot / 64] |= bit;
                    }
                }
                if let Some(register) = operands::writes(instruction) {
                    charge(work, 1)?;
                    let slot = register.index();
                    touch(&mut ranges[slot], pc);
                    block.defs[slot / 64] |= 1 << (slot % 64);
                }
            }
            let mut successor = |target: usize| {
                block.successors.push(
                    leaders
                        .binary_search(&target)
                        .expect("verified branch leader"),
                );
            };
            match code.get(end.checked_sub(1)?)? {
                BytecodeInstruction::Jump { target } => successor(target.index()),
                BytecodeInstruction::Branch {
                    then_target,
                    else_target,
                    ..
                } => {
                    successor(then_target.index());
                    successor(else_target.index());
                }
                BytecodeInstruction::Return(_) | BytecodeInstruction::Unreachable => {}
                _ if end < code.len() => successor(end),
                _ => return None,
            }
            blocks.push(block);
        }
        loop {
            let mut changed = false;
            for index in (0..blocks.len()).rev() {
                for word in 0..words {
                    charge(work, 1 + blocks[index].successors.len())?;
                    let out = blocks[index]
                        .successors
                        .iter()
                        .fold(0, |out, &s| out | blocks[s].live[word]);
                    let block = &mut blocks[index];
                    let live = block.uses[word] | (out & !block.defs[word]);
                    changed |= live != block.live[word];
                    block.live[word] = live;
                    block.out[word] = out;
                }
            }
            if !changed {
                break;
            }
        }
        for block in &blocks {
            for (word, (&live, &out)) in block.live.iter().zip(&block.out).enumerate() {
                charge(work, 1)?;
                for (bits, pc) in [(live, block.start), (out, block.end - 1)] {
                    let mut bits = bits;
                    while bits != 0 {
                        charge(work, 1)?;
                        let bit = bits.trailing_zeros() as usize;
                        touch(&mut ranges[word * 64 + bit], pc);
                        bits &= bits - 1;
                    }
                }
            }
        }
        let mut intervals = ranges
            .iter()
            .enumerate()
            .filter_map(|(slot, &range)| range.map(|(start, end)| (start, end, slot)))
            .collect::<Vec<_>>();
        intervals.sort_unstable();
        let mut active = Vec::<(usize, usize)>::new();
        let mut free = Vec::new();
        let mut slots = vec![0; count];
        let mut allocated = 0;
        for (start, end, logical) in intervals {
            charge(work, active.len() + 1)?;
            active.retain(|&(last, physical)| {
                if last < start {
                    free.push(physical);
                    false
                } else {
                    true
                }
            });
            let physical = free.pop().unwrap_or_else(|| {
                let slot = allocated;
                allocated += 1;
                slot
            });
            slots[logical] = physical;
            active.push((end, physical));
        }
        // A declared but unused register is never executed. Keep a valid slot
        // for defensive raw frame inspection without adding per-slot storage.
        if count != 0 {
            allocated = allocated.max(1);
        }
        Some(Self {
            slots: slots.into(),
            count: allocated,
        })
    }
}

fn touch(range: &mut Option<(usize, usize)>, pc: usize) {
    match range {
        Some((start, end)) => {
            *start = (*start).min(pc);
            *end = (*end).max(pc);
        }
        None => *range = Some((pc, pc)),
    }
}

fn charge(work: &mut usize, amount: usize) -> Option<()> {
    *work = work.saturating_add(amount);
    (*work <= MAX_WORK).then_some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kagari_bytecode::instruction::{
        BinaryOp, ConstantOperand, JumpTarget, LocalSlot, Register,
    };

    #[test]
    fn loop_carried_values_do_not_alias_body_temporaries() {
        let r = Register::new;
        let function = BytecodeFunction {
            register_count: 5,
            instructions: vec![
                BytecodeInstruction::LoadConst {
                    dst: r(0),
                    constant: ConstantOperand::I32(0),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(1),
                    constant: ConstantOperand::I32(3),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(2),
                    constant: ConstantOperand::I32(1),
                },
                BytecodeInstruction::Binary {
                    dst: r(3),
                    op: BinaryOp::Add,
                    lhs: r(0),
                    rhs: r(2),
                },
                BytecodeInstruction::Move {
                    dst: r(0),
                    src: r(3),
                },
                BytecodeInstruction::Binary {
                    dst: r(4),
                    op: BinaryOp::Lt,
                    lhs: r(0),
                    rhs: r(1),
                },
                BytecodeInstruction::Branch {
                    cond: r(4),
                    then_target: JumpTarget::new(2),
                    else_target: JumpTarget::new(7),
                },
                BytecodeInstruction::Return(Some(r(0))),
            ],
            ..Default::default()
        };
        let allocation = RegisterAllocation::prepare(&function, &mut 0);
        assert_eq!(allocation.count, 4);
        for live in 0..2 {
            for temporary in 2..5 {
                assert_ne!(allocation.slots[live], allocation.slots[temporary]);
            }
        }
        assert_ne!(allocation.slots[2], allocation.slots[3]);
    }

    #[test]
    fn backedge_liveness_extends_a_definition_past_its_last_textual_use() {
        let r = Register::new;
        let function = BytecodeFunction {
            register_count: 4,
            instructions: vec![
                BytecodeInstruction::LoadLocal {
                    dst: r(0),
                    local: LocalSlot::new(0),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(1),
                    constant: ConstantOperand::I32(7),
                },
                BytecodeInstruction::Move {
                    dst: r(2),
                    src: r(1),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(3),
                    constant: ConstantOperand::I32(9),
                },
                BytecodeInstruction::Branch {
                    cond: r(0),
                    then_target: JumpTarget::new(2),
                    else_target: JumpTarget::new(5),
                },
                BytecodeInstruction::Return(Some(r(2))),
            ],
            ..Default::default()
        };
        let allocation = RegisterAllocation::prepare(&function, &mut 0);
        assert_ne!(allocation.slots[1], allocation.slots[3]);
        assert_ne!(allocation.slots[0], allocation.slots[3]);
        assert_ne!(allocation.slots[2], allocation.slots[3]);
    }
}
