//! Suspension safety comes from checked flow and live slots, never a supplied flag.
use crate::{
    analysis::FunctionAnalysis,
    function::MirFunction,
    instruction::Instruction,
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error, analysis::Budget},
};
use kagari_abi::representation::ValueType;
use kagari_common::identity::table::DefinitionId;
use kagari_contract::contracts::ContractError;
use kagari_types::ty::Ty;

pub(super) fn ephemeral(physical: ValueType, semantic: Option<&Ty<DefinitionId>>) -> bool {
    physical == ValueType::HostHandle || semantic.is_some_and(inline_host)
}

fn inline_host(ty: &Ty<DefinitionId>) -> bool {
    match ty {
        Ty::Host(_) => true,
        Ty::Tuple(items) => items.iter().any(inline_host),
        _ => false,
    }
}

pub(super) fn verify(
    function: &MirFunction,
    analysis: &FunctionAnalysis,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<(), MirVerificationError> {
    if !function.effects.may_suspend {
        return Ok(());
    }
    let invalid = |context: Context<'_>, reason| {
        context.error(Error::Contract(ContractError::InvalidOperation { reason }))
    };
    let mut visited = vec![false; function.blocks.len()];
    visited[function.entry.index()] = true;
    let mut pending = vec![function.entry];
    while let Some(id) = pending.pop() {
        let block = &function.blocks[id.index()];
        let facts = analysis.block(id).expect("verified analysis");
        for (index, instruction) in block.instructions.iter().enumerate() {
            let context = Context {
                block: Some(id),
                instruction: Some(index),
                span: Some(block.instruction_spans[index]),
                ..context
            };
            budget.work(1, context)?;
            if let Instruction::Await { .. } = instruction {
                let live = facts.instruction(index).expect("instruction facts").live();
                budget.work(function.temps.len() + function.locals.len(), context)?;
                let unsafe_temp = live.temps().any(|temp| {
                    ephemeral(
                        function.temps[temp.index()].ty,
                        function.semantic.registers.get(&temp.index()),
                    )
                });
                let unsafe_local = live.locals().any(|local| {
                    ephemeral(
                        function.locals[local.index()].ty,
                        function.semantic.locals.get(&local.index()),
                    )
                });
                if unsafe_temp || unsafe_local {
                    return Err(invalid(context, "host capability live across await"));
                }
            }
        }
        for successor in block
            .terminator
            .as_ref()
            .expect("verified terminator")
            .successors()
        {
            budget.work(1, context)?;
            if !visited[successor.index()] {
                visited[successor.index()] = true;
                pending.push(successor);
            }
        }
    }
    Ok(())
}
