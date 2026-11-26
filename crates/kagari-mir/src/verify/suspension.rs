//! Retention checks use executable producers and independently derived liveness.
mod projections;
#[cfg(test)]
mod tests;
use crate::{
    analysis::FunctionAnalysis,
    function::MirFunction,
    instruction::{Constant, Instruction},
    verify::{Context, MirVerificationError, MirVerificationErrorKind as Error, analysis::Budget},
};
use kagari_abi::representation::ValueType;
use kagari_contract::contracts::ContractError;
use kagari_types::ty::Ty;
use projections::{Projections, Shape};

// A host-typed heap slot can be a captured cell. At publication and before
// parking, execution must still check actual values, including tuple contents,
// and active loans/guards: a parameter's Host annotation proves no lifetime.
pub(super) fn invalid_retained_slot(physical: ValueType, semantic: Option<&Ty>) -> bool {
    match (physical, semantic) {
        (ValueType::HostHandle | ValueType::HeapObject, Some(Ty::Host(_))) => false,
        (ValueType::HostHandle, _) | (_, Some(Ty::Host(_))) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct SlotFact {
    view: Shape,
    index: Option<usize>,
}

pub(super) fn verify(
    function: &MirFunction,
    analysis: &FunctionAnalysis,
    has_path_view: bool,
    context: Context<'_>,
    budget: &mut Budget,
) -> Result<(), MirVerificationError> {
    if !function.effects.may_suspend && !has_path_view {
        return Ok(());
    }
    let invalid = |context: Context<'_>, reason| {
        context.error(Error::Contract(ContractError::InvalidOperation { reason }))
    };
    let temps = function.temps.len();
    let slots = temps + function.locals.len();
    let width = if has_path_view { slots } else { 0 };
    let mut bytes = width
        .saturating_mul(function.blocks.len() + 1)
        .saturating_mul(size_of::<SlotFact>());
    budget.scratch(bytes, context)?;
    let mut shapes = Projections::default();
    let mut entries = vec![vec![SlotFact::default(); width]; function.blocks.len()];
    let mut reached = vec![false; function.blocks.len()];
    reached[function.entry.index()] = true;
    let mut queued = vec![false; function.blocks.len()];
    let mut pending = vec![function.entry];
    while let Some(id) = pending.pop() {
        queued[id.index()] = false;
        budget.work(width + 1, context)?;
        let mut views = entries[id.index()].clone();
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
                budget.work(slots, context)?;
                if live.temps().any(|temp| {
                    (has_path_view && views[temp.index()].view.contains_view())
                        || invalid_retained_slot(
                            function.temps[temp.index()].ty,
                            function.semantic.registers.get(&temp.index()),
                        )
                }) || live.locals().any(|local| {
                    (has_path_view && views[temps + local.index()].view.contains_view())
                        || invalid_retained_slot(
                            function.locals[local.index()].ty,
                            function.semantic.locals.get(&local.index()),
                        )
                }) {
                    return Err(invalid(context, "host capability live across await"));
                }
            }
            if has_path_view {
                if let Instruction::MakeFuture { arguments, .. }
                | Instruction::MakeClosure {
                    captures: arguments,
                    ..
                } = instruction
                {
                    budget.work(arguments.len(), context)?;
                    if arguments
                        .iter()
                        .any(|arg| views[arg.temp.index()].view.contains_view())
                    {
                        return Err(invalid(context, "cannot capture a host path view"));
                    }
                }
                transfer(instruction, temps, &mut views, &mut shapes, &mut |w, b| {
                    budget.projection(&mut bytes, w, b, context)
                })?;
            }
        }
        for successor in block
            .terminator
            .as_ref()
            .expect("verified terminator")
            .successors()
        {
            budget.work(width + 1, context)?;
            let next = successor.index();
            let mut changed = !reached[next];
            if reached[next] {
                for (entry, out) in entries[next].iter_mut().zip(&views) {
                    let view = shapes.join(entry.view, out.view, &mut |w, b| {
                        budget.projection(&mut bytes, w, b, context)
                    })?;
                    let index = entry.index.filter(|index| out.index == Some(*index));
                    let merged = SlotFact { view, index };
                    changed |= *entry != merged;
                    *entry = merged;
                }
            } else {
                entries[next].clone_from_slice(&views);
                reached[next] = true;
            }
            if changed && !queued[next] {
                pending.push(successor);
                queued[next] = true;
            }
        }
    }
    Ok(())
}

fn transfer(
    instruction: &Instruction,
    temps: usize,
    views: &mut [SlotFact],
    shapes: &mut Projections,
    charge: &mut impl FnMut(usize, usize) -> Result<(), MirVerificationError>,
) -> Result<(), MirVerificationError> {
    let (dst, view) = match instruction {
        Instruction::StoreLocal { local, src } => (temps + local.index(), views[src.temp.index()]),
        _ => {
            let Some(dst) = instruction.output() else {
                return Ok(());
            };
            let mut view = SlotFact::default();
            match instruction {
                Instruction::MakePathView { .. } => view.view = Shape::VIEW,
                Instruction::Move { src, .. } => view = views[src.temp.index()],
                Instruction::LoadLocal { local, .. } => view = views[temps + local.index()],
                Instruction::LoadConst { constant, .. } => {
                    view.index = match constant {
                        Constant::I32(n) => usize::try_from(*n).ok(),
                        Constant::I64(n) => usize::try_from(*n).ok(),
                        Constant::U64(n) => usize::try_from(*n).ok(),
                        _ => None,
                    }
                }
                Instruction::MakeTuple { elements, .. } => {
                    charge(elements.len(), 0)?;
                    view.view = shapes.tuple(
                        elements.iter().map(|arg| views[arg.temp.index()].view),
                        charge,
                    )?;
                }
                Instruction::ReadAggregateIndex { base, index, .. } => {
                    view.view = shapes.project(
                        views[base.temp.index()].view,
                        views[index.temp.index()].index,
                        charge,
                    )?;
                }
                _ => {}
            };
            (dst.temp.index(), view)
        }
    };
    views[dst] = view;
    Ok(())
}
