//! Optional tail facts never grant authority to bypass the ordinary call contract.
use crate::{
    instruction::{BytecodeInstruction as I, CallTarget},
    module::{BytecodeFunction, BytecodeModule},
    program::BytecodeProgram,
    verifier::BytecodeVerificationError as Error,
};
use kagari_abi::representation::ValueType;
use kagari_common::cancellation::CancellationToken;
use std::collections::HashSet;

pub(super) fn verify(
    module: &BytecodeModule,
    program: Option<&BytecodeProgram>,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    if module
        .functions
        .iter()
        .all(|f| f.metadata.self_tail_calls.is_empty())
    {
        return Ok(());
    }
    let mut excluded: HashSet<_> = module
        .module_slots
        .iter()
        .filter(|slot| slot.contract.supplier == module.identity)
        .map(|slot| slot.initializer)
        .collect();
    for instruction in module.functions.iter().flat_map(|f| &f.instructions) {
        cancel.check().map_err(|_| Error::Cancelled)?;
        if let I::MakeFuture { function, .. } | I::MakeClosure { function, .. } = instruction {
            excluded.insert(*function);
        }
    }
    for function in &module.functions {
        if function.metadata.self_tail_calls.is_empty() {
            continue;
        }
        let invalid = || Error::InvalidOperation {
            function: function.id,
            reason: "invalid scalar self-tail candidate",
        };
        let metadata = &function.metadata;
        if excluded.contains(&function.id)
            || metadata.effects.may_suspend
            || metadata.semantic.generic.is_some()
            || metadata.semantic.protocol_adapter.is_some()
            || !scalar(metadata.return_type)
            || metadata
                .params
                .iter()
                .chain(&metadata.slots)
                .any(|ty| !scalar(*ty))
            || metadata
                .self_tail_calls
                .windows(2)
                .any(|pcs| pcs[0] >= pcs[1])
        {
            return Err(invalid());
        }
        // Inspect actual edges, not an untrusted optional target summary. A
        // skipped Return cannot also be an independently entered block.
        let mut targets = HashSet::new();
        for instruction in &function.instructions {
            cancel.check().map_err(|_| Error::Cancelled)?;
            match instruction {
                I::Jump { target } => {
                    targets.insert(target.index());
                }
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
                    targets.insert(then_target.index());
                    targets.insert(else_target.index());
                }
                _ => {}
            }
        }
        for &pc in &metadata.self_tail_calls {
            cancel.check().map_err(|_| Error::Cancelled)?;
            let next = pc.checked_add(1).ok_or_else(invalid)?;
            if targets.contains(&next) || !terminal_pair(module, function, program, pc) {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

fn terminal_pair(
    module: &BytecodeModule,
    function: &BytecodeFunction,
    program: Option<&BytecodeProgram>,
    pc: usize,
) -> bool {
    let Some(I::Call { dst, callee, .. }) = function.instructions.get(pc) else {
        return false;
    };
    let Some(I::Return(result)) = pc
        .checked_add(1)
        .and_then(|pc| function.instructions.get(pc))
    else {
        return false;
    };
    if dst != result {
        return false;
    }
    match callee {
        CallTarget::Function(id) => *id == function.id,
        CallTarget::ModuleFunction {
            module: owner,
            function: id,
        } => {
            *id == function.id
                && program
                    .and_then(|p| p.modules.get(owner.index()))
                    .is_some_and(|owner| owner.identity == module.identity)
        }
        _ => false,
    }
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
