use crate::{
    ids::BlockId,
    passes::Work,
    verify::{MirVerificationError, VerifiedMirModule},
};
use kagari_abi::effects::EffectSet;

/// A backwards local sweep uses sealed live-outs, preserving cross-block uses.
/// Skipping operands of removed operations also removes dead producer chains in
/// the same block. It deliberately does not delete blocks or their charge points.
pub(super) fn find(
    module: &VerifiedMirModule,
    work: &mut Work<'_>,
) -> Result<Vec<(usize, usize, usize)>, MirVerificationError> {
    let mut removals = Vec::new();
    for (function_index, function) in module.functions.iter().enumerate() {
        let facts = module.analysis(function.id).expect("sealed function");
        for (block_index, block) in function.blocks.iter().enumerate() {
            let facts = facts
                .block(BlockId::new(block_index))
                .expect("sealed block");
            if !facts.reachable() {
                continue;
            }
            work.charge(function.temps.len())?;
            let mut live = vec![false; function.temps.len()];
            for temp in facts.terminator().live().temps() {
                live[temp.index()] = true;
            }
            for (index, instruction) in block.instructions.iter().enumerate().rev() {
                work.charge(1)?;
                if let Some(dst) = instruction.output() {
                    if !live[dst.temp.index()] && instruction.effects() == EffectSet::default() {
                        removals.push((function_index, block_index, index));
                        continue;
                    }
                    live[dst.temp.index()] = false;
                }
                let inputs = instruction.inputs();
                work.charge(inputs.len())?;
                for value in inputs {
                    live[value.temp.index()] = true;
                }
            }
        }
    }
    Ok(removals)
}
