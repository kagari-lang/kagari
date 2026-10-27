//! Select scalar homes without changing snapshots, checked commits or observations.
use crate::bytecode::placement::{MAX_WORK, PlacementFacts, TransferSelection, scalar};
use kagari_bytecode::{
    instruction::{BytecodeInstruction as I, Slot},
    module::BytecodeFunction,
};

struct Roles {
    definitions: Vec<Vec<usize>>,
    uses: Vec<Vec<usize>>,
}

pub(super) fn select(
    function: &mut BytecodeFunction,
    facts: &PlacementFacts<'_>,
    selection: &mut TransferSelection<'_>,
) {
    let Some(roles) = collect_roles(function, selection) else {
        return;
    };
    initialize_homes(function, facts, selection, &roles);
    let Some(roles) = collect_roles(function, selection) else {
        return;
    };
    select_destinations(function, facts, selection, &roles);
    let Some(roles) = collect_roles(function, selection) else {
        return;
    };
    forward_returns(function, facts, selection, &roles);
    let Some(roles) = collect_roles(function, selection) else {
        return;
    };
    for (pc, instruction) in function.instructions.iter().enumerate() {
        if !charge(selection, 1) {
            return;
        }
        if let I::LoadConst { dst, .. } = instruction
            && dst.index() >= facts.homes
            && scalar(function.metadata.slots[dst.index()])
            && roles.uses[dst.index()].is_empty()
        {
            selection.retained[pc] = false;
        }
    }
}

fn charge(selection: &mut TransferSelection<'_>, amount: usize) -> bool {
    *selection.work = selection.work.saturating_add(amount);
    *selection.work <= MAX_WORK
}

fn collect_roles(
    function: &BytecodeFunction,
    selection: &mut TransferSelection<'_>,
) -> Option<Roles> {
    let count = function.metadata.slots.len();
    if !charge(selection, count) {
        return None;
    }
    let mut roles = Roles {
        definitions: vec![Vec::new(); count],
        uses: vec![Vec::new(); count],
    };
    for (pc, instruction) in function.instructions.iter().enumerate() {
        if !charge(selection, 1) {
            return None;
        }
        if !selection.retained[pc] {
            continue;
        }
        if let Some(dst) = instruction.slot_output() {
            roles.definitions[dst.index()].push(pc);
        }
        for slot in instruction.slot_inputs() {
            if !charge(selection, 1) {
                return None;
            }
            roles.uses[slot.index()].push(pc);
        }
    }
    Some(roles)
}

fn same_scalar(function: &BytecodeFunction, a: Slot, b: Slot) -> bool {
    scalar(function.metadata.slots[a.index()])
        && function.metadata.slots[a.index()] == function.metadata.slots[b.index()]
        && function.metadata.semantic.slots.contains_key(&a.index())
        && function.metadata.semantic.slots.get(&a.index())
            == function.metadata.semantic.slots.get(&b.index())
}

fn initialize_homes(
    function: &mut BytecodeFunction,
    facts: &PlacementFacts<'_>,
    selection: &mut TransferSelection<'_>,
    roles: &Roles,
) {
    for pc in 0..function.instructions.len() {
        if !charge(selection, 1) {
            return;
        }
        let I::Move { dst, src } = function.instructions[pc] else {
            continue;
        };
        if !selection.retained[pc]
            || dst.index() >= facts.homes
            || src.index() < facts.homes
            || !same_scalar(function, dst, src)
        {
            continue;
        }
        let [producer] = roles.definitions[src.index()].as_slice() else {
            continue;
        };
        if *producer >= pc || facts.blocks[*producer] != facts.blocks[pc] {
            continue;
        }
        if let I::LoadConst { constant, .. } = function.instructions[*producer] {
            // Scalar literals have no allocation or callback. Keep each home write
            // at its original assignment point, with that point's source origin.
            function.instructions[pc] = I::LoadConst { dst, constant };
        }
    }
}

fn select_destinations(
    function: &mut BytecodeFunction,
    facts: &PlacementFacts<'_>,
    selection: &mut TransferSelection<'_>,
    roles: &Roles,
) {
    for pc in 0..function.instructions.len().saturating_sub(1) {
        if !charge(selection, 1) {
            return;
        }
        if !selection.retained[pc] || !selection.retained[pc + 1] {
            continue;
        }
        let Some(result) = function.instructions[pc].slot_output() else {
            continue;
        };
        let I::Move { dst, src } = function.instructions[pc + 1] else {
            continue;
        };
        if src != result
            || dst.index() >= facts.homes
            || result.index() < facts.homes
            || roles.definitions[result.index()] != [pc]
            || !same_scalar(function, dst, result)
            || facts.points[pc].debug_available() != facts.points[pc + 1].debug_available()
            || function.metadata.origins[pc].span != function.metadata.origins[pc + 1].span
            || function.instructions[pc]
                .slot_inputs()
                .iter()
                .any(|slot| !scalar(function.metadata.slots[slot.index()]))
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
        let uses = &roles.uses[result.index()];
        let Some(&last) = uses.last() else {
            continue;
        };
        if !charge(selection, uses.len() + last.saturating_sub(pc)) {
            return;
        }
        if uses
            .iter()
            .any(|&use_pc| use_pc <= pc || facts.blocks[use_pc] != facts.blocks[pc])
            || (pc + 2..=last).any(|next| {
                selection.retained[next]
                    && (facts.barriers[next]
                        || function.instructions[next].slot_output() == Some(dst)
                        || facts.points[next].debug_available()
                            != facts.points[pc].debug_available())
            })
        {
            continue;
        }
        function.instructions[pc].rewrite_slots(|slot| if slot == result { dst } else { slot });
        for &use_pc in uses {
            if use_pc != pc + 1 && selection.retained[use_pc] {
                function.instructions[use_pc]
                    .rewrite_slots(|slot| if slot == result { dst } else { slot });
            }
        }
        selection.retained[pc + 1] = false;
        selection.targets[pc + 1] = pc;
    }
}

fn forward_returns(
    function: &mut BytecodeFunction,
    facts: &PlacementFacts<'_>,
    selection: &mut TransferSelection<'_>,
    roles: &Roles,
) {
    for pc in 0..function.instructions.len().saturating_sub(1) {
        if !charge(selection, 1) {
            return;
        }
        let I::Move { dst, src } = function.instructions[pc] else {
            continue;
        };
        if selection.retained[pc]
            && selection.retained[pc + 1]
            && dst.index() >= facts.homes
            && same_scalar(function, dst, src)
            && roles.definitions[dst.index()] == [pc]
            && roles.uses[dst.index()] == [pc + 1]
            && facts.blocks[pc] == facts.blocks[pc + 1]
            && matches!(function.instructions[pc + 1], I::Return(Some(slot)) if slot == dst)
        {
            if !charge(selection, function.metadata.debug.local_live_ranges.len()) {
                return;
            }
            function.instructions[pc + 1] = I::Return(Some(src));
            // The removed tail read was a statement observation. Its live names
            // remain inspectable at the return that now owns that source point.
            for range in &mut function.metadata.debug.local_live_ranges {
                if range.start <= pc && range.end == pc + 1 {
                    range.end = pc + 2;
                }
            }
            selection.retained[pc] = false;
        }
    }
}

pub(super) fn remove_fallthroughs(
    function: &BytecodeFunction,
    selection: &mut TransferSelection<'_>,
) {
    if !charge(selection, function.instructions.len()) {
        return;
    }
    let mut next = vec![function.instructions.len(); function.instructions.len() + 1];
    for pc in (0..function.instructions.len()).rev() {
        if !charge(selection, 1) {
            return;
        }
        if !selection.retained[pc] {
            next[pc] = next[pc + 1];
            continue;
        }
        if let I::Jump { target } = function.instructions[pc]
            && target.index() > pc
            && next[target.index()] == next[pc + 1]
        {
            selection.retained[pc] = false;
            next[pc] = next[pc + 1];
        } else {
            next[pc] = pc;
        }
    }
}
