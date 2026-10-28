//! Bounded operand and control selection with preserved source observations.
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{
        BinaryOp, BytecodeInstruction as I, ConstantOperand, ScalarOperand, Slot, UnaryOp,
    },
    module::BytecodeFunction,
};
use kagari_types::ty::Ty;
use std::ops::Range;

const MAX_WORK: usize = 4_000_000;

fn roles(function: &BytecodeFunction, retained: &[bool]) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut definitions = vec![Vec::new(); function.metadata.slots.len()];
    let mut uses = definitions.clone();
    for (pc, instruction) in function
        .instructions
        .iter()
        .enumerate()
        .filter(|(pc, _)| retained[*pc])
    {
        if let Some(dst) = instruction.slot_output() {
            definitions[dst.index()].push(pc);
        }
        for input in instruction.slot_inputs() {
            uses[input.index()].push(pc);
        }
    }
    (definitions, uses)
}

fn numeric(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::I32 | ValueType::I64 | ValueType::U64 | ValueType::F32 | ValueType::F64
    )
}

fn accepts_constant(instruction: &I, function: &BytecodeFunction) -> bool {
    match instruction {
        I::Numeric { rhs: Some(_), .. } => true,
        I::Binary { op, lhs, rhs, .. } => {
            !matches!(op, BinaryOp::IdentityEq | BinaryOp::IdentityNotEq)
                && numeric(function.metadata.slots[lhs.index()])
                && numeric(function.metadata.slots[rhs.index()])
        }
        I::BinaryOperands { .. } => true,
        _ => false,
    }
}

/// A retained home initialization can own the literal's original observation.
fn constant_observation(
    function: &BytecodeFunction,
    interval: Range<usize>,
    blocks: &[usize],
    homes: usize,
    retained: &[bool],
    debug_compatible: &impl Fn(usize, usize) -> bool,
    work: &mut usize,
) -> Option<usize> {
    let pc = interval.start;
    let I::LoadConst {
        dst: source,
        constant,
    } = function.instructions[pc]
    else {
        return None;
    };
    for next in pc + 1..interval.end {
        *work = work.saturating_add(1);
        if *work > MAX_WORK || blocks[next] != blocks[pc] || !debug_compatible(pc, next) {
            return None;
        }
        if retained[next]
            && let I::LoadConst {
                dst,
                constant: other,
            } = function.instructions[next]
            && dst.index() < homes
            && constant == other
            && function.metadata.semantic.slots.get(&source.index())
                == function.metadata.semantic.slots.get(&dst.index())
            && function.metadata.origins[pc].span == function.metadata.origins[next].span
        {
            return Some(next);
        }
        if retained[next] {
            return None;
        }
    }
    None
}

fn replace_operand(instruction: &mut I, source: Slot, constant: ScalarOperand) {
    let rewrite = |slot| {
        if slot == source {
            constant
        } else {
            ScalarOperand::Slot(slot)
        }
    };
    *instruction = match *instruction {
        I::Binary { dst, op, lhs, rhs } => I::BinaryOperands {
            dst,
            op,
            lhs: rewrite(lhs),
            rhs: rewrite(rhs),
        },
        I::Numeric {
            dst,
            operation,
            lhs,
            rhs: Some(rhs),
        } => I::BinaryOperands {
            dst,
            op: BinaryOp::Numeric(operation),
            lhs: rewrite(lhs),
            rhs: rewrite(rhs),
        },
        I::BinaryOperands { dst, op, lhs, rhs } => I::BinaryOperands {
            dst,
            op,
            lhs: if lhs.slot() == Some(source) {
                constant
            } else {
                lhs
            },
            rhs: if rhs.slot() == Some(source) {
                constant
            } else {
                rhs
            },
        },
        _ => unreachable!("eligible constant use"),
    };
}

pub(super) fn select(
    function: &mut BytecodeFunction,
    constants: &[ConstantOperand],
    blocks: &[usize],
    homes: usize,
    retained: &mut [bool],
    targets: &mut [usize],
    debug_compatible: impl Fn(usize, usize) -> bool,
) {
    let (definitions, uses) = roles(function, retained);
    let mut work = 0usize;
    for pc in 0..retained.len() {
        if !retained[pc] {
            continue;
        }
        let I::LoadConst { dst, constant } = function.instructions[pc] else {
            continue;
        };
        let Some(Ty::Builtin(ty)) = function.metadata.semantic.slots.get(&dst.index()) else {
            continue;
        };
        if dst.index() < homes
            || definitions[dst.index()] != [pc]
            || uses[dst.index()].is_empty()
            || constants[constant.index()].numeric_payload(*ty).is_none()
        {
            continue;
        }
        let uses = &uses[dst.index()];
        work = work.saturating_add(uses.len());
        if work > MAX_WORK {
            break;
        }
        if uses
            .iter()
            .any(|&next| next <= pc || !accepts_constant(&function.instructions[next], function))
        {
            continue;
        }
        let observation = constant_observation(
            function,
            pc..uses[0],
            blocks,
            homes,
            retained,
            &debug_compatible,
            &mut work,
        );
        let last = *uses.last().expect("nonempty uses");
        work = work.saturating_add(last - pc);
        if work > MAX_WORK {
            break;
        }
        if observation.is_none()
            && (uses.iter().any(|&next| blocks[next] != blocks[pc])
                || !(pc..=last).all(|next| debug_compatible(pc, next)))
        {
            continue;
        }
        let operand = ScalarOperand::Constant { constant, ty: *ty };
        for &next in uses {
            replace_operand(&mut function.instructions[next], dst, operand);
        }
        retained[pc] = false;
        if let Some(observation) = observation {
            targets[pc] = observation;
        }
    }
    let (definitions, uses) = roles(function, retained);
    for branch in 0..retained.len() {
        if !retained[branch] {
            continue;
        }
        let I::Branch {
            mut cond,
            then_target,
            else_target,
        } = function.instructions[branch]
        else {
            continue;
        };
        let mut consumer = branch;
        let mut negated = false;
        let mut removed = Vec::new();
        loop {
            work = work.saturating_add(1);
            if work > MAX_WORK || cond.index() < homes || uses[cond.index()] != [consumer] {
                break;
            }
            let [producer] = definitions[cond.index()].as_slice() else {
                break;
            };
            let producer = *producer;
            if producer >= consumer || blocks[producer] != blocks[branch] {
                break;
            }
            work = work.saturating_add(consumer - producer);
            if work > MAX_WORK
                || (producer + 1..consumer).any(|pc| retained[pc])
                || !(producer..=consumer).all(|pc| debug_compatible(producer, pc))
            {
                break;
            }
            match function.instructions[producer] {
                I::Unary {
                    op: UnaryOp::Not,
                    operand,
                    ..
                } => {
                    removed.push(producer);
                    consumer = producer;
                    cond = operand;
                    negated = !negated;
                }
                I::Binary { op, lhs, rhs, .. } => {
                    if !predicate(op) {
                        break;
                    }
                    function.instructions[producer] = I::PredicateBranch {
                        op,
                        lhs: ScalarOperand::Slot(lhs),
                        rhs: ScalarOperand::Slot(rhs),
                        negated,
                        then_target,
                        else_target,
                    };
                    removed.push(branch);
                    for pc in removed {
                        retained[pc] = false;
                        targets[pc] = producer;
                    }
                    break;
                }
                I::BinaryOperands { op, lhs, rhs, .. } => {
                    if !predicate(op) {
                        break;
                    }
                    function.instructions[producer] = I::PredicateBranch {
                        op,
                        lhs,
                        rhs,
                        negated,
                        then_target,
                        else_target,
                    };
                    removed.push(branch);
                    for pc in removed {
                        retained[pc] = false;
                        targets[pc] = producer;
                    }
                    break;
                }
                _ => break,
            }
        }
    }
}

fn predicate(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::IdentityEq
            | BinaryOp::IdentityNotEq
            | BinaryOp::Lt
            | BinaryOp::Le
            | BinaryOp::Gt
            | BinaryOp::Ge
    )
}
