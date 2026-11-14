//! Candidate facts come from checked MIR; ordinary instructions remain intact.
use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, CallTarget},
    module::BytecodeFunction,
};
use kagari_contract::ids::FunctionRef;
use kagari_mir::{
    function::MirFunction,
    instruction::{CallTarget as MirCallTarget, Instruction, Terminator},
};
use std::collections::HashSet;

/// Closure/resume and static-initializer entries retain their ordinary protocol.
pub(super) fn exclude_entries(
    functions: &mut [BytecodeFunction],
    initializers: impl Iterator<Item = FunctionRef>,
) {
    let mut excluded: HashSet<_> = initializers.collect();
    for instruction in functions.iter().flat_map(|f| &f.instructions) {
        if let BytecodeInstruction::MakeFuture { function, .. }
        | BytecodeInstruction::MakeClosure { function, .. } = instruction
        {
            excluded.insert(*function);
        }
    }
    for function in functions {
        if excluded.contains(&function.id) {
            function.metadata.self_tail_calls.clear();
        }
    }
}

pub(super) fn candidates(function: &MirFunction) -> Vec<usize> {
    if function.effects.may_suspend
        || function.semantic.generic.is_some()
        || function.semantic.protocol_adapter.is_some()
        || !scalar(function.return_type)
        || function.locals.iter().any(|local| !scalar(local.ty))
        || function.temps.iter().any(|temp| !scalar(temp.ty))
    {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut pc = 0;
    for (_, block) in function.emission_order() {
        if let (Some(Instruction::Call { dst, callee, .. }), Some(Terminator::Return(result))) =
            (block.instructions.last(), &block.terminator)
            && dst == result
            && match callee {
                MirCallTarget::Function(id) => *id == function.id,
                MirCallTarget::SourceFunction(contract) => {
                    contract.declaration == function.instance.declaration
                        && contract.arguments == function.instance.arguments
                }
                _ => false,
            }
        {
            candidates.push(pc + block.instructions.len() - 1);
        }
        pc += block.instructions.len() + usize::from(block.terminator.is_some());
    }
    candidates
}

/// Placement maps original PCs once. It must still leave the exact terminal
/// pair; removing/fusing a later operation cannot create a new candidate.
pub(super) fn terminal_pair(instructions: &[BytecodeInstruction], pc: usize) -> bool {
    matches!(
        instructions.get(pc..pc.saturating_add(2)),
        Some([BytecodeInstruction::Call { dst, callee: CallTarget::Function(_)
            | CallTarget::ModuleFunction { .. }, .. }, BytecodeInstruction::Return(result)])
            if dst == result
    )
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
