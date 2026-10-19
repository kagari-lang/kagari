//! Conservative interpreter placement changes storage, not semantic MIR.
mod selection;

use crate::bytecode::collect_control_flow_targets;
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, ConstantOperand, JumpTarget, Slot},
    module::{BytecodeFunction, BytecodeSemanticSlots, RootSlotLayout},
};
use kagari_contract::{contracts, operations::BinaryOp};
use kagari_mir::{
    analysis::FunctionAnalysis,
    function::MirFunction,
    ids::BlockId,
    instruction::{Instruction, MirValue},
};
use kagari_types::{scalar::BuiltinType, ty::Ty};

const MAX_WORK: usize = 4_000_000;

/// Project checked contracts for generated scalar values that have no source expression.
/// In particular, compound assignment emits its local read/result without a HIR
/// expression annotation. Never infer a narrow type from its shared physical tag.
pub(super) fn project_scalar_facts(mir: &MirFunction, semantic: &mut BytecodeSemanticSlots) {
    let homes = mir.locals.len();
    let mut definitions = vec![0; mir.temps.len()];
    for instruction in mir.blocks.iter().flat_map(|block| &block.instructions) {
        if let Some(output) = instruction.output() {
            definitions[output.temp.index()] += 1;
        }
    }
    for (_, block) in mir.emission_order() {
        for instruction in &block.instructions {
            let Some(output) = instruction.output() else {
                continue;
            };
            let slot = homes + output.temp.index();
            if definitions[output.temp.index()] != 1
                || !scalar(output.ty)
                || semantic.slots.contains_key(&slot)
            {
                continue;
            }
            let value_type = |value: &MirValue| semantic.slots.get(&(homes + value.temp.index()));
            let ty = match instruction {
                Instruction::LoadLocal { local, .. } => semantic.slots.get(&local.index()).cloned(),
                Instruction::Move { src, .. } => value_type(src).cloned(),
                Instruction::Numeric { operation, .. }
                | Instruction::Binary {
                    op: BinaryOp::Numeric(operation),
                    ..
                } => operation.contract().map(|signature| signature.output),
                Instruction::Convert { conversion, .. } => {
                    conversion.contract().map(|(_, output)| output)
                }
                Instruction::Binary { op, lhs, rhs, .. } if value_type(lhs) == value_type(rhs) => {
                    let result = contracts::binary_result(*op, lhs.ty, rhs.ty).ok();
                    checked_scalar_result(value_type(lhs), result)
                }
                Instruction::Unary { op, operand, .. } => checked_scalar_result(
                    value_type(operand),
                    contracts::unary_result(*op, operand.ty).ok(),
                ),
                _ => None,
            };
            if let Some(ty) = ty {
                semantic.slots.insert(slot, ty);
            }
        }
    }
}

fn checked_scalar_result(input: Option<&Ty>, output: Option<ValueType>) -> Option<Ty> {
    let Ty::Builtin(input) = input? else {
        return None;
    };
    if !matches!(
        input,
        BuiltinType::Unit
            | BuiltinType::Bool
            | BuiltinType::I32
            | BuiltinType::I64
            | BuiltinType::U64
            | BuiltinType::F32
            | BuiltinType::F64
    ) {
        return None;
    }
    match output? {
        ValueType::Bool => Some(Ty::Builtin(BuiltinType::Bool)),
        _ => Some(Ty::Builtin(*input)),
    }
}

pub(super) fn select(
    mir: &MirFunction,
    analysis: &FunctionAnalysis,
    function: &mut BytecodeFunction,
    constants: &[ConstantOperand],
) {
    let count = function.metadata.slots.len();
    let mut definitions = vec![0usize; count];
    let mut uses = vec![Vec::new(); count];
    let mut block_of = vec![0; function.instructions.len()];
    let mut points = Vec::new();
    let mut barriers = Vec::new();
    for (id, block) in mir.emission_order() {
        let facts = analysis.block(BlockId::new(id)).expect("sealed block");
        for index in 0..block.instructions.len() {
            block_of[points.len()] = id;
            points.push(facts.instruction(index).expect("sealed point"));
            let effects = block.instructions[index].effects();
            barriers.push(effects.calls || effects.may_suspend);
        }
        block_of[points.len()] = id;
        points.push(facts.terminator());
        barriers.push(false);
    }
    for (pc, instruction) in function.instructions.iter().enumerate() {
        for input in instruction.slot_inputs() {
            uses[input.index()].push(pc);
        }
        if let Some(output) = instruction.slot_output() {
            definitions[output.index()] += 1;
        }
    }
    let mut retained = vec![true; function.instructions.len()];
    let mut targets = (0..retained.len()).collect::<Vec<_>>();
    let mut work = 0usize;
    for (id, block) in mir.emission_order() {
        let facts = analysis.block(BlockId::new(id)).expect("sealed block");
        let start = facts.start_offset();
        let end = start + block.instructions.len() + 1;
        for (index, instruction) in block.instructions.iter().enumerate() {
            let pc = start + index;
            work = work.saturating_add(1);
            if work > MAX_WORK {
                break;
            }
            let Instruction::LoadLocal { dst, local } = instruction else {
                continue;
            };
            let temp = mir.locals.len() + dst.temp.index();
            let home = local.index();
            if definitions[temp] != 1
                || !scalar(dst.ty)
                || !function.metadata.semantic.slots.contains_key(&temp)
                || function.metadata.semantic.slots.get(&temp)
                    != function.metadata.semantic.slots.get(&home)
                || uses[temp].is_empty()
                || uses[temp]
                    .iter()
                    .any(|&use_pc| block_of[use_pc] != id || use_pc <= pc)
            {
                continue;
            }
            let last = *uses[temp].last().expect("nonempty uses");
            work = work.saturating_add(last - pc);
            if work > MAX_WORK {
                break;
            }
            let stable = (pc + 1..=last).all(|next| {
                let operation = &function.instructions[next];
                let barrier = next < last
                    && (operation.slot_output() == Some(Slot::new(home)) || barriers[next]);
                !barrier && points[next].debug_available() == points[pc].debug_available()
            });
            if !stable {
                continue;
            }
            for &use_pc in &uses[temp] {
                function.instructions[use_pc].rewrite_slots(|slot| {
                    if slot.index() == temp {
                        Slot::new(home)
                    } else {
                        slot
                    }
                });
            }
            retained[pc] = false;
        }
        for pc in start..end - 1 {
            if !retained[pc] || !retained[pc + 1] {
                continue;
            }
            let Some(result) = function.instructions[pc].slot_output() else {
                continue;
            };
            let I::Move { dst, src } = function.instructions[pc + 1] else {
                continue;
            };
            if src != result
                || dst.index() >= mir.locals.len()
                || result.index() < mir.locals.len()
                || definitions[result.index()] != 1
                || uses[result.index()] != [pc + 1]
                || !scalar(function.metadata.slots[dst.index()])
                || function.instructions[pc]
                    .slot_inputs()
                    .iter()
                    .any(|input| !scalar(function.metadata.slots[input.index()]))
                || !function
                    .metadata
                    .semantic
                    .slots
                    .contains_key(&result.index())
                || function.metadata.semantic.slots.get(&result.index())
                    != function.metadata.semantic.slots.get(&dst.index())
                || points[pc].debug_available() != points[pc + 1].debug_available()
                || function.metadata.origins[pc].span != function.metadata.origins[pc + 1].span
                || !matches!(
                    function.instructions[pc],
                    I::LoadConst { .. }
                        | I::Move { .. }
                        | I::Unary { .. }
                        | I::Binary { .. }
                        | I::Numeric { .. }
                        | I::Convert { .. }
                )
            {
                continue;
            }
            function.instructions[pc].rewrite_slots(|slot| if slot == result { dst } else { slot });
            retained[pc + 1] = false;
            targets[pc + 1] = pc;
        }
    }
    selection::select(
        function,
        constants,
        &block_of,
        mir.locals.len(),
        &mut retained,
        &mut targets,
        |from, to| points[from].debug_available() == points[to].debug_available(),
    );
    let mut pcs = vec![0; retained.len() + 1];
    let mut next = 0;
    for (pc, &keep) in retained.iter().enumerate() {
        pcs[pc] = next;
        next += usize::from(keep);
    }
    pcs[retained.len()] = next;
    for origin in &mut function.metadata.origins {
        origin.instruction = pcs[targets[origin.instruction]];
    }
    let mut origins_per_pc = vec![0usize; next];
    for origin in &function.metadata.origins {
        origins_per_pc[origin.instruction] += 1;
    }
    function.metadata.instruction_origins = retained
        .iter()
        .enumerate()
        .filter_map(|(origin, &keep)| (keep && origins_per_pc[pcs[origin]] > 1).then_some(origin))
        .collect();
    let mut selected = Vec::with_capacity(next);
    for (pc, mut instruction) in function.instructions.drain(..).enumerate() {
        if !retained[pc] {
            continue;
        }
        match &mut instruction {
            I::Jump { target } => *target = JumpTarget::new(pcs[target.index()]),
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
                *then_target = JumpTarget::new(pcs[then_target.index()]);
                *else_target = JumpTarget::new(pcs[else_target.index()]);
            }
            _ => {}
        }
        selected.push(instruction);
    }
    function.instructions = selected;
    let debug = &mut function.metadata.debug;
    for span in &mut debug.source_spans {
        span.instruction_offset = pcs[targets[span.instruction_offset]];
    }
    for line in &mut debug.line_table {
        line.instruction_offset = pcs[targets[line.instruction_offset]];
    }
    for point in &mut debug.safe_debug_points {
        point.instruction_offset = pcs[targets[point.instruction_offset]];
    }
    for range in &mut debug.local_live_ranges {
        range.start = pcs[range.start];
        range.end = pcs[range.end];
    }
    debug
        .local_live_ranges
        .retain(|range| range.start < range.end);
    let mut used = vec![false; count];
    used[..mir.locals.len()].fill(true);
    for instruction in &function.instructions {
        for input in instruction.slot_inputs() {
            used[input.index()] = true;
        }
        if let Some(output) = instruction.slot_output() {
            used[output.index()] = true;
        }
    }
    let mut slots = Vec::new();
    let mut renumber = vec![Slot::new(0); count];
    for (old, &keep) in used.iter().enumerate() {
        if keep {
            renumber[old] = Slot::new(slots.len());
            slots.push(function.metadata.slots[old]);
        }
    }
    for instruction in &mut function.instructions {
        instruction.rewrite_slots(|slot| renumber[slot.index()]);
    }
    let semantic = &mut function.metadata.semantic.slots;
    *semantic = semantic
        .iter()
        .filter(|(old, _)| used[**old])
        .map(|(&old, ty)| (renumber[old].index(), ty.clone()))
        .collect();
    function.metadata.slots = slots;
    function.metadata.roots = RootSlotLayout::from_types(&function.metadata.slots);
    function.metadata.control_flow_targets = collect_control_flow_targets(&function.instructions);
    function.metadata.debug.frame_layout.slots = function.metadata.slots.clone();
    function.slot_count = u32::try_from(function.metadata.slots.len()).expect("bounded MIR slots");
}

fn scalar(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::Unit
            | ValueType::Bool
            | ValueType::I32
            | ValueType::I64
            | ValueType::U64
            | ValueType::F32
            | ValueType::F64
    )
}
