//! Physical slot intervals consume verified control flow and binding availability.
//! Logical identities remain stable; source names do not require a second bank.
use crate::module::execution::layout::scalar_type;
use kagari_bytecode::{
    availability::DebugAvailability,
    instruction::{BytecodeInstruction, Slot},
    module::BytecodeFunction,
};
use std::collections::BTreeSet;

const MAX_WORK: usize = 4_000_000;
const MAX_WORDS: usize = 1_000_000;

/// Managed locations are distinct and never participate in scalar coloring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SlotColor {
    Scalar(usize),
    Managed,
}

#[derive(Debug)]
pub(super) struct SlotAllocation {
    pub slots: Box<[SlotColor]>,
    pub scalar_count: usize,
    pub ranges: Box<[Option<(usize, usize)>]>,
    pub ends_at_write: Box<[bool]>,
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
            // This mandatory output construction remains linear even after the
            // optimization budget is exhausted; account for its classification.
            let _ = charge(work, function.slot_count as usize);
            let mut scalar_count = 0;
            let slots = function
                .metadata
                .slots
                .iter()
                .map(|&representation| {
                    if scalar_type(representation).is_some() {
                        let color = SlotColor::Scalar(scalar_count);
                        scalar_count += 1;
                        color
                    } else {
                        SlotColor::Managed
                    }
                })
                .collect();
            Self {
                slots,
                scalar_count,
                ranges: Box::default(),
                ends_at_write: Box::default(),
            }
        })
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
                .saturating_add(count.saturating_mul(size_of::<SlotColor>().div_ceil(8)))
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
        // Only closed scalar handlers consume every input before a fallible
        // computation and commit the output after success. Keep named homes and
        // parameters at closed endpoints for debugger and public entry access.
        let mut early_ends = vec![false; count];
        let mut named = vec![false; count];
        for range in &function.metadata.debug.local_live_ranges {
            charge(work, 1)?;
            named[range.slot.index()] = true;
        }
        for slot in &function.metadata.parameter_slots {
            named[slot.index()] = true;
        }
        for (pc, instruction) in code.iter().enumerate() {
            charge(work, 1)?;
            if !read_before_write(instruction, function) {
                continue;
            }
            let dst = instruction
                .slot_output()
                .expect("closed scalar destination");
            for input in instruction.slot_inputs() {
                charge(work, 1)?;
                if input != dst
                    && !named[input.index()]
                    && ranges[input.index()].is_some_and(|(_, end)| end == pc)
                {
                    early_ends[input.index()] = true;
                }
            }
        }
        charge(work, count)?;
        let mut has_scalar = false;
        let mut slots: Vec<_> = function
            .metadata
            .slots
            .iter()
            .map(|&representation| {
                if scalar_type(representation).is_some() {
                    has_scalar = true;
                    SlotColor::Scalar(0)
                } else {
                    SlotColor::Managed
                }
            })
            .collect();
        let mut intervals = ranges
            .iter()
            .enumerate()
            .filter(|&(slot, _)| matches!(slots[slot], SlotColor::Scalar(_)))
            .filter_map(|(slot, &range)| range.map(|(start, end)| (start, end, slot)))
            .collect::<Vec<_>>();
        intervals.sort_unstable();
        let mut active = Vec::<(usize, usize, usize)>::new();
        let mut free = Vec::new();
        let mut allocated = 0;
        let mut ends_at_write = vec![false; count];
        for (start, end, logical) in intervals {
            charge(work, active.len() + 1)?;
            active.retain(|&(last, physical, _)| {
                if last < start {
                    free.push(physical);
                    false
                } else {
                    true
                }
            });
            // Prefer a fully dead color. Mark a consumed endpoint only when its
            // physical location is actually selected for this result.
            let physical = free
                .pop()
                .or_else(|| {
                    let index = active.iter().position(|&(last, _, previous)| {
                        last == start
                            && early_ends[previous]
                            && code[start]
                                .slot_output()
                                .is_some_and(|slot| slot.index() == logical)
                    })?;
                    let (_, physical, previous) = active.swap_remove(index);
                    ends_at_write[previous] = true;
                    Some(physical)
                })
                .unwrap_or_else(|| {
                    let slot = allocated;
                    allocated += 1;
                    slot
                });
            slots[logical] = SlotColor::Scalar(physical);
            active.push((end, physical, logical));
        }
        // Unused scalar registers keep a valid color for defensive raw frame
        // inspection. Managed-only functions need no scalar storage at all.
        if has_scalar {
            allocated = allocated.max(1);
        }
        Some(Self {
            slots: slots.into(),
            scalar_count: allocated,
            ranges: ranges.into(),
            ends_at_write: ends_at_write.into(),
        })
    }
}

fn read_before_write(instruction: &BytecodeInstruction, function: &BytecodeFunction) -> bool {
    matches!(
        instruction,
        BytecodeInstruction::Move { .. }
            | BytecodeInstruction::Unary { .. }
            | BytecodeInstruction::Binary { .. }
            | BytecodeInstruction::BinaryOperands { .. }
            | BytecodeInstruction::Numeric { .. }
            | BytecodeInstruction::Convert { .. }
    ) && instruction
        .slot_output()
        .is_some_and(|dst| scalar_type(function.metadata.slots[dst.index()]).is_some())
        && instruction
            .slot_inputs()
            .iter()
            .all(|slot| scalar_type(function.metadata.slots[slot.index()]).is_some())
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
        assert_eq!(fallback.slots.as_ref(), &[SlotColor::Scalar(0)]);
    }

    #[test]
    fn overlapping_debug_ranges_prevent_reuse_until_the_last_range_ends() {
        let mut function = BytecodeFunction {
            instructions: vec![
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(0),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Move {
                    dst: Slot::new(0),
                    src: Slot::new(0),
                },
                BytecodeInstruction::Jump {
                    target: JumpTarget::new(3),
                },
                BytecodeInstruction::LoadConst {
                    dst: Slot::new(1),
                    constant: ConstantId::new(0),
                },
                BytecodeInstruction::Return(Some(Slot::new(1))),
            ],
            metadata: FunctionMetadata {
                slots: vec![ValueType::I32; 2],
                ..Default::default()
            },
            slot_count: 2,
            ..Default::default()
        };
        let without_debug = SlotAllocation::prepare(&function, &mut 0);
        assert_eq!(without_debug.ranges.as_ref(), &[Some((0, 1)), Some((3, 4))]);
        assert_eq!(without_debug.slots[0], without_debug.slots[1]);
        assert_eq!(without_debug.scalar_count, 1);

        function.metadata.debug.local_live_ranges = [(1, 3), (2, 4)]
            .into_iter()
            .enumerate()
            .map(|(binding, (start, end))| LocalLiveRange {
                slot: Slot::new(0),
                binding: binding as u32,
                name: String::new(),
                span: Default::default(),
                start,
                end,
                ty: ValueType::I32,
                is_parameter: false,
            })
            .collect();
        let with_debug = SlotAllocation::prepare(&function, &mut 0);
        assert_eq!(with_debug.ranges.as_ref(), &[Some((0, 3)), Some((3, 4))]);
        assert_ne!(with_debug.slots[0], with_debug.slots[1]);
        assert_eq!(with_debug.scalar_count, 2);
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
        assert_eq!(allocation.scalar_count, 3);
        for live in 0..2 {
            for temporary in 2..5 {
                assert_ne!(allocation.slots[live], allocation.slots[temporary]);
            }
        }
        assert_eq!(
            allocation.slots[2], allocation.slots[3],
            "the dying scalar input is read before its result replaces it"
        );
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
