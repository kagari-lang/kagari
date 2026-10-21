//! Physical slot intervals consume verified control flow and binding availability.
//! Logical identities remain stable; source names do not require a second bank.
use kagari_bytecode::{
    availability::DebugAvailability,
    instruction::{BytecodeInstruction, Slot},
    module::BytecodeFunction,
};
use std::collections::BTreeSet;

const MAX_WORK: usize = 4_000_000;
const MAX_WORDS: usize = 1_000_000;

#[derive(Debug)]
pub(crate) struct SlotAllocation {
    pub slots: Box<[usize]>,
    pub count: usize,
    pub ranges: Box<[Option<(usize, usize)>]>,
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

impl SlotAllocation {
    pub(super) fn prepare(function: &BytecodeFunction, work: &mut usize) -> Self {
        Self::allocate(function, work).unwrap_or_else(|| {
            // Bounded preparation may decline allocation without changing code
            // semantics or accepting unchecked data. This is not an old format.
            let count = function.slot_count as usize;
            Self {
                slots: (0..count).collect(),
                count,
                ranges: Box::default(),
            }
        })
    }

    pub(crate) fn index(&self, logical: usize) -> Option<usize> {
        self.slots.get(logical).copied()
    }

    fn allocate(function: &BytecodeFunction, work: &mut usize) -> Option<Self> {
        let code = &function.instructions;
        let count = function.slot_count as usize;
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
                }
                | BytecodeInstruction::PredicateBranch {
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
        let debug_bytes =
            DebugAvailability::scratch_bytes(function.metadata.debug.local_live_ranges.len());
        if leaders.len() > 16384
            || words
                .saturating_mul(leaders.len())
                .saturating_mul(4)
                .saturating_add(debug_bytes.div_ceil(8))
                > MAX_WORDS
        {
            return None;
        }
        let availability =
            DebugAvailability::new(&function.metadata.debug.local_live_ranges, |amount| {
                charge(work, amount).ok_or(())
            })
            .ok()?;
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
                let mut input = |register: Slot| {
                    let slot = register.index();
                    touch(&mut ranges[slot], pc);
                    let bit = 1 << (slot % 64);
                    if block.defs[slot / 64] & bit == 0 {
                        block.uses[slot / 64] |= bit;
                    }
                };
                availability
                    .visit(pc, |amount| charge(work, amount).ok_or(()), &mut input)
                    .ok()?;
                for register in instruction.slot_inputs() {
                    charge(work, 1)?;
                    input(register);
                }
                if let Some(register) = instruction.slot_output() {
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
                }
                | BytecodeInstruction::PredicateBranch {
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
        for slot in &function.metadata.parameter_slots {
            touch(&mut ranges[slot.index()], 0);
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
            ranges: ranges.into(),
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
    use kagari_abi::representation::ValueType;
    use kagari_bytecode::{
        instruction::{BinaryOp, ConstantId, JumpTarget, Slot},
        module::{FunctionMetadata, LocalLiveRange},
    };

    #[test]
    fn sparse_debug_ranges_preserve_reuse_and_budget_fallback() {
        let slot = Slot::new(0);
        let mut instructions = vec![
            BytecodeInstruction::Move {
                dst: slot,
                src: slot
            };
            9_999
        ];
        instructions.push(BytecodeInstruction::Return(Some(slot)));
        let mut metadata = FunctionMetadata {
            slots: vec![ValueType::I32],
            parameter_slots: vec![slot],
            ..Default::default()
        };
        metadata.debug.local_live_ranges = (0..500)
            .map(|i| LocalLiveRange {
                slot,
                binding: i as u32,
                name: String::new(),
                span: Default::default(),
                start: i * 20,
                end: i * 20 + 1,
                ty: ValueType::I32,
                is_parameter: true,
            })
            .collect();
        let function = BytecodeFunction {
            instructions,
            metadata,
            slot_count: 1,
            ..Default::default()
        };
        let mut work = 0;
        let allocation = SlotAllocation::prepare(&function, &mut work);
        assert_eq!(allocation.ranges.as_ref(), &[Some((0, 9_999))]);
        assert!(work < MAX_WORK);
        let mut exhausted = MAX_WORK;
        let fallback = SlotAllocation::prepare(&function, &mut exhausted);
        assert!(fallback.ranges.is_empty());
        assert_eq!(fallback.slots.as_ref(), &[0]);
    }

    #[test]
    fn loop_carried_values_do_not_alias_body_temporaries() {
        let r = Slot::new;
        let function = {
            let fixture_instructions = vec![
                BytecodeInstruction::LoadConst {
                    dst: r(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(1),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(2),
                    constant: ConstantId::new(0),
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
            ];
            let mut fixture_metadata = kagari_bytecode::module::FunctionMetadata {
                slots: vec![kagari_abi::representation::ValueType::I32; 5],
                ..Default::default()
            };

            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
                ..Default::default()
            }
        };
        let allocation = SlotAllocation::prepare(&function, &mut 0);
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
        let r = Slot::new;
        let function = {
            let fixture_instructions = vec![
                BytecodeInstruction::Move {
                    dst: r(0),
                    src: Slot::new(0),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(1),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Move {
                    dst: r(2),
                    src: r(1),
                },
                BytecodeInstruction::LoadConst {
                    dst: r(3),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Branch {
                    cond: r(0),
                    then_target: JumpTarget::new(2),
                    else_target: JumpTarget::new(5),
                },
                BytecodeInstruction::Return(Some(r(2))),
            ];
            let mut fixture_metadata = kagari_bytecode::module::FunctionMetadata {
                slots: vec![kagari_abi::representation::ValueType::I32; 4],
                ..Default::default()
            };

            if fixture_metadata.origins.is_empty() {
                fixture_metadata.origins = (0..fixture_instructions.len())
                    .map(|instruction| kagari_bytecode::module::InstructionOrigin {
                        span: Default::default(),
                        line: None,
                        column: None,
                        instruction,
                    })
                    .collect();
            }
            BytecodeFunction {
                slot_count: u32::try_from(fixture_metadata.slots.len()).unwrap(),
                metadata: fixture_metadata,
                instructions: fixture_instructions,
                ..Default::default()
            }
        };
        let allocation = SlotAllocation::prepare(&function, &mut 0);
        assert_ne!(allocation.slots[1], allocation.slots[3]);
        assert_ne!(allocation.slots[0], allocation.slots[3]);
        assert_ne!(allocation.slots[2], allocation.slots[3]);
    }
}
