//! Duplicate bounded closed scalar conditions into dominated jump backedges.
//!
//! The original header remains the entry guard. Fresh temporary identities keep
//! condition snapshots separate, and ordinary verification rebuilds all analyses.
use crate::{
    function::{BasicBlock, MirFunction, MirModule, MirTemp},
    ids::TempId,
    instruction::{Instruction, MirValue, Terminator},
    passes::Work,
    verify::MirVerificationError,
};
use kagari_abi::representation::ValueType;
use std::collections::BTreeMap;

const MAX_CONDITION: usize = 16;
const MAX_DUPLICATED: usize = 256;

pub(super) fn apply(
    module: &mut MirModule,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    for function in &mut module.functions {
        rotate(function, work)?;
    }
    Ok(())
}

fn rotate(function: &mut MirFunction, work: &mut Work<'_>) -> Result<(), MirVerificationError> {
    if function.effects.may_suspend {
        return Ok(());
    }
    work.charge(function.blocks.len() + function.temps.len())?;
    let mut predecessors = vec![Vec::new(); function.blocks.len()];
    for (index, block) in function.blocks.iter().enumerate() {
        work.charge(1)?;
        if let Some(Terminator::Jump(target)) = block.terminator {
            predecessors[target.index()].push(index);
        }
    }
    let headers = function
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(index, block)| {
            (!predecessors[index].is_empty()
                && matches!(block.terminator, Some(Terminator::Branch { .. }))
                && !block.instructions.is_empty()
                && block.instructions.len() <= MAX_CONDITION)
                .then_some(index)
        })
        .collect::<Vec<_>>();
    if headers.is_empty() {
        return Ok(());
    }
    let initial_reachable = reachable(function, None, work)?;
    let mut definitions = vec![0usize; function.temps.len()];
    let mut users = vec![Vec::new(); function.temps.len()];
    for (index, block) in function.blocks.iter().enumerate() {
        for instruction in &block.instructions {
            work.charge(1 + instruction.inputs().len())?;
            if let Some(dst) = instruction.output() {
                definitions[dst.temp.index()] += 1;
            }
            for input in instruction.inputs() {
                users[input.temp.index()].push(index);
            }
        }
        if let Some(input) = block.terminator.as_ref().and_then(Terminator::input) {
            users[input.temp.index()].push(index);
        }
    }
    let mut duplicated = 0;
    let mut conditions = Vec::new();
    for header in headers {
        if closed_condition(&function.blocks[header], header, &definitions, &users, work)? {
            conditions.push((header, function.blocks[header].clone()));
        }
    }
    for (header, condition) in conditions {
        // A reachable predecessor that cannot be reached while avoiding the
        // header is dominated by it. Only its Jump edge is tail-duplicated.
        let bypass = reachable(function, Some(header), work)?;
        let mut latches = Vec::new();
        for &index in &predecessors[header] {
            work.charge(1)?;
            if initial_reachable[index] && !bypass[index] {
                latches.push(index);
            }
        }
        let growth = condition.instructions.len().saturating_mul(latches.len());
        if latches.is_empty() || growth > MAX_DUPLICATED - duplicated {
            continue;
        }
        for latch in latches {
            append_condition(function, latch, &condition, work)?;
        }
        duplicated += growth;
    }
    Ok(())
}

fn reachable(
    function: &MirFunction,
    excluded: Option<usize>,
    work: &mut Work<'_>,
) -> Result<Vec<bool>, MirVerificationError> {
    work.charge(function.blocks.len())?;
    let mut visited = vec![false; function.blocks.len()];
    let mut pending = vec![function.entry];
    while let Some(block) = pending.pop() {
        work.charge(1)?;
        if Some(block.index()) == excluded || visited[block.index()] {
            continue;
        }
        visited[block.index()] = true;
        pending.extend(
            function.blocks[block.index()]
                .terminator
                .as_ref()
                .expect("verified MIR terminator")
                .successors(),
        );
    }
    Ok(visited)
}

fn scalar(ty: ValueType) -> bool {
    matches!(
        ty,
        ValueType::Unit
            | ValueType::Bool
            | ValueType::Char
            | ValueType::I32
            | ValueType::I64
            | ValueType::U64
            | ValueType::F32
            | ValueType::F64
    )
}

fn closed_condition(
    block: &BasicBlock,
    header: usize,
    definitions: &[usize],
    users: &[Vec<usize>],
    work: &mut Work<'_>,
) -> Result<bool, MirVerificationError> {
    let mut outputs = BTreeMap::new();
    for instruction in &block.instructions {
        work.charge(1 + instruction.inputs().len())?;
        if !matches!(
            instruction,
            Instruction::LoadConst { .. }
                | Instruction::LoadLocal { .. }
                | Instruction::Move { .. }
                | Instruction::Unary { .. }
                | Instruction::Binary { .. }
                | Instruction::Numeric { .. }
                | Instruction::Convert { .. }
        ) || instruction
            .inputs()
            .iter()
            .any(|input| !outputs.contains_key(&input.temp.index()))
        {
            return Ok(false);
        }
        let Some(dst) = instruction.output() else {
            return Ok(false);
        };
        work.charge(users[dst.temp.index()].len())?;
        if !scalar(dst.ty)
            || definitions[dst.temp.index()] != 1
            || users[dst.temp.index()].iter().any(|&block| block != header)
        {
            return Ok(false);
        }
        outputs.insert(dst.temp.index(), dst.ty);
    }
    Ok(block
        .terminator
        .as_ref()
        .and_then(Terminator::input)
        .is_some_and(|cond| outputs.contains_key(&cond.temp.index())))
}

fn append_condition(
    function: &mut MirFunction,
    latch: usize,
    condition: &BasicBlock,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    let mut replacements = BTreeMap::new();
    let mut instructions = Vec::with_capacity(condition.instructions.len());
    for original in &condition.instructions {
        work.charge(1 + original.inputs().len())?;
        let old = original.output().expect("closed scalar condition output");
        let dst = MirValue {
            temp: TempId::new(function.temps.len()),
            ty: old.ty,
        };
        function.temps.push(MirTemp { ty: old.ty });
        if let Some(ty) = function.semantic.registers.get(&old.temp.index()).cloned() {
            function.semantic.registers.insert(dst.temp.index(), ty);
        }
        let mut instruction = original.clone();
        instruction.rewrite_inputs(|input| *input = replacements[&input.temp.index()]);
        match &mut instruction {
            Instruction::LoadConst { dst: output, .. }
            | Instruction::LoadLocal { dst: output, .. }
            | Instruction::Move { dst: output, .. }
            | Instruction::Unary { dst: output, .. }
            | Instruction::Binary { dst: output, .. }
            | Instruction::Numeric { dst: output, .. }
            | Instruction::Convert { dst: output, .. } => *output = dst,
            _ => unreachable!("closed scalar condition"),
        }
        replacements.insert(old.temp.index(), dst);
        instructions.push(instruction);
    }
    let block = &mut function.blocks[latch];
    block.instructions.extend(instructions);
    block
        .instruction_spans
        .extend_from_slice(&condition.instruction_spans);
    block
        .instruction_scopes
        .extend_from_slice(&condition.instruction_scopes);
    block.terminator = condition.terminator.clone();
    block
        .terminator
        .as_mut()
        .expect("condition branch")
        .rewrite_input(|input| *input = replacements[&input.temp.index()]);
    block.terminator_span = condition.terminator_span;
    block.terminator_scope = condition.terminator_scope;
    Ok(())
}
