//! Forward immutable temporary values through copies and local bindings. Heap
//! loads, module reads and cell reads remain observable operations. Stores to
//! locals are retained for debugger availability; no mutable referent is folded.
use crate::{
    function::{MirFunction, MirModule},
    ids::TempId,
    instruction::{Constant, Instruction, MirValue},
    passes::{PassStatistics, Work},
    verify::MirVerificationError,
};
use std::{collections::HashMap, mem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ConstantKey {
    Unit,
    Bool(bool),
    I32(i32),
    I64(i64),
    U64(u64),
    F32(u32),
    F64(u64),
}

impl ConstantKey {
    fn from_constant(value: &Constant) -> Option<Self> {
        Some(match value {
            Constant::Unit => Self::Unit,
            Constant::Bool(v) => Self::Bool(*v),
            Constant::I32(v) => Self::I32(*v),
            Constant::I64(v) => Self::I64(*v),
            Constant::U64(v) => Self::U64(*v),
            Constant::F32(v) => Self::F32(v.to_bits()),
            Constant::F64(v) => Self::F64(v.to_bits()),
            Constant::Str(_) => return None,
        })
    }
}

pub(super) fn simplify(
    module: &mut MirModule,
    statistics: &mut PassStatistics,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    for function in &mut module.functions {
        simplify_function(function, statistics, work)?;
    }
    Ok(())
}

fn simplify_function(
    function: &mut MirFunction,
    statistics: &mut PassStatistics,
    work: &mut Work<'_>,
) -> Result<(), MirVerificationError> {
    work.charge(function.temps.len())?;
    let mut definitions = vec![0usize; function.temps.len()];
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        work.charge(1)?;
        if let Some(dst) = instruction.output() {
            definitions[dst.temp.index()] += 1;
        }
    }
    let mut aliases = function
        .temps
        .iter()
        .enumerate()
        .map(|(index, temp)| MirValue {
            temp: TempId::new(index),
            ty: temp.ty,
        })
        .collect::<Vec<_>>();
    let semantic = &function.semantic.registers;
    let same = |a: MirValue, b: MirValue| {
        a.ty == b.ty && semantic.get(&a.temp.index()) == semantic.get(&b.temp.index())
    };
    let mut constants = HashMap::new();
    let mut entry_constants = Vec::new();
    let order = function
        .emission_order()
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    for index in order {
        let block = &mut function.blocks[index];
        work.charge(function.locals.len())?;
        let mut locals = vec![None; function.locals.len()];
        let mut retained = Vec::with_capacity(block.instructions.len());
        let mut spans = Vec::with_capacity(block.instructions.len());
        let mut scopes = Vec::with_capacity(block.instructions.len());
        for ((mut instruction, span), scope) in mem::take(&mut block.instructions)
            .into_iter()
            .zip(mem::take(&mut block.instruction_spans))
            .zip(mem::take(&mut block.instruction_scopes))
        {
            work.charge(1 + instruction.inputs().len())?;
            instruction.rewrite_inputs(|value| *value = aliases[value.temp.index()]);
            let mut remove = false;
            match &instruction {
                Instruction::LoadConst { dst, constant } if definitions[dst.temp.index()] == 1 => {
                    if let Some(key) = ConstantKey::from_constant(constant) {
                        let key = (key, semantic.get(&dst.temp.index()).cloned());
                        if let Some(previous) = constants.get(&key) {
                            aliases[dst.temp.index()] = *previous;
                            statistics.constants_reused += 1;
                        } else {
                            constants.insert(key, *dst);
                            // Scalar constants neither trap nor allocate script objects.
                            // Put a single definition in the dominating entry block.
                            if index != function.entry.index() {
                                entry_constants.push((instruction.clone(), span));
                            }
                        }
                        remove =
                            aliases[dst.temp.index()] != *dst || index != function.entry.index();
                    }
                }
                Instruction::LoadLocal { dst, local } if definitions[dst.temp.index()] == 1 => {
                    if let Some(previous) = locals[local.index()]
                        && same(*dst, previous)
                    {
                        aliases[dst.temp.index()] = previous;
                        statistics.copies_removed += 1;
                        remove = true;
                    }
                    locals[local.index()] = Some(aliases[dst.temp.index()]);
                }
                Instruction::StoreLocal { local, src } => {
                    locals[local.index()] = (definitions[src.temp.index()] == 1).then_some(*src);
                }
                Instruction::Move { dst, src }
                    if definitions[dst.temp.index()] == 1
                        && definitions[src.temp.index()] == 1
                        && same(*dst, *src) =>
                {
                    aliases[dst.temp.index()] = *src;
                    statistics.copies_removed += 1;
                    remove = true;
                }
                _ => {}
            }
            if instruction.effects().calls {
                work.charge(locals.len())?;
                locals.fill(None);
            }
            if !remove {
                retained.push(instruction);
                spans.push(span);
                scopes.push(scope);
            }
        }
        block.instructions = retained;
        block.instruction_spans = spans;
        block.instruction_scopes = scopes;
    }
    // A later block may reuse an earlier constant before that definition's old
    // location. All constants now dominate every block; resolve transitive aliases.
    for index in 0..aliases.len() {
        let mut value = aliases[index];
        while aliases[value.temp.index()].temp != value.temp {
            work.charge(1)?;
            value = aliases[value.temp.index()];
        }
        aliases[index] = value;
    }
    for block in &mut function.blocks {
        for instruction in &mut block.instructions {
            work.charge(1 + instruction.inputs().len())?;
            instruction.rewrite_inputs(|value| *value = aliases[value.temp.index()]);
        }
        work.charge(1)?;
        if let Some(terminator) = &mut block.terminator {
            terminator.rewrite_input(|value| *value = aliases[value.temp.index()]);
        }
    }
    let entry = &mut function.blocks[function.entry.index()];
    let count = entry_constants.len();
    for (instruction, span) in entry_constants {
        entry.instructions.push(instruction);
        entry.instruction_spans.push(span);
    }
    entry.instruction_scopes.extend(vec![0; count]);
    Ok(())
}
