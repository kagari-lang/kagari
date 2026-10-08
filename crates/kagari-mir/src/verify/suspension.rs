//! Suspension safety comes from checked flow and live slots, never a supplied flag.
use crate::{
    analysis::FunctionAnalysis,
    function::MirFunction,
    ids::BlockId,
    instruction::Instruction,
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error, analysis::Budget},
};
use kagari_abi::representation::ValueType;
use kagari_contract::contracts::ContractError;
use kagari_types::ty::Ty;

fn ephemeral(physical: ValueType, semantic: Option<&Ty>) -> bool {
    physical == ValueType::HostHandle || semantic.is_some_and(inline_host)
}

fn inline_host(ty: &Ty) -> bool {
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
    let mut depths = vec![None; function.blocks.len()];
    depths[function.entry.index()] = Some(0usize);
    let mut pending = vec![function.entry];
    while let Some(id) = pending.pop() {
        let mut depth = depths[id.index()].expect("queued block");
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
            match instruction {
                Instruction::BeginIteration { .. } => depth += 1,
                Instruction::EndIteration => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| invalid(context, "iteration resource underflow"))?;
                }
                Instruction::Await { .. } => {
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
                _ => {}
            }
        }
        // Return/trap unwinds all owned iteration leases. Normal CFG joins must
        // have the same stack shape, including loop backedges and continue edges.
        for successor in block
            .terminator
            .as_ref()
            .expect("verified terminator")
            .successors()
        {
            budget.work(1, context)?;
            match depths[successor.index()] {
                None => {
                    depths[successor.index()] = Some(depth);
                    pending.push(successor);
                }
                Some(previous) if previous != depth => {
                    return Err(invalid(
                        Context {
                            block: Some(BlockId::new(successor.index())),
                            ..context
                        },
                        "inconsistent iteration resource stack",
                    ));
                }
                Some(_) => {}
            }
        }
    }
    Ok(())
}
