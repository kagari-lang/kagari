//! Bounded scalar simplification and dead pure-operation cleanup.
//!
//! These passes preserve effect order and source provenance. Dead operations and
//! their parallel span/scope entries are deleted; fresh verification rebuilds
//! logical offsets, roots, safepoints and debug facts. No charge-only operations
//! survive. Pass work limits bound compilation, not script execution.
use kagari_common::cancellation::CancellationToken;

use crate::verify::{
    MirVerificationError, MirVerificationErrorKind, VerifiedMirModule, verify_mir,
};
mod constants;
mod dead;
mod scalar;

#[derive(Debug, Clone)]
pub struct PassOptions {
    /// Visits, operand accesses and temporary-state initialization across both passes.
    /// MIR verification/analysis additionally enforces its own resource bounds.
    pub max_work: usize,
}

impl Default for PassOptions {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PassStatistics {
    pub constants_folded: usize,
    pub branches_simplified: usize,
    pub dead_operations_removed: usize,
}

#[derive(Debug)]
pub struct PassResult {
    pub module: VerifiedMirModule,
    pub statistics: PassStatistics,
}

/// Consume the old seal. Intermediate edits cannot be passed to a backend, and
/// successful results contain fresh verification, liveness, roots and debug facts.
/// Cancellation or resource exhaustion returns no partially transformed module.
pub fn optimize(
    module: VerifiedMirModule,
    options: &PassOptions,
    cancel: &CancellationToken,
) -> Result<PassResult, MirVerificationError> {
    let mut work = Work {
        used: 0,
        limit: options.max_work,
        cancel,
    };
    work.charge(0)?;
    let mut statistics = PassStatistics::default();
    let mut raw = module.to_unverified(cancel)?;
    constants::simplify(&mut raw, &mut statistics, &mut work)?;
    let module = verify_mir(raw, cancel)?;
    let removals = dead::find(&module, &mut work)?;
    statistics.dead_operations_removed = removals.len();
    let module = if removals.is_empty() {
        module
    } else {
        let mut raw = module.to_unverified(cancel)?;
        // Remove backwards within each block so original indices remain valid.
        let mut removals = removals;
        removals.sort_unstable_by(|left, right| right.cmp(left));
        for (function, block, instruction) in removals {
            work.charge(1)?;
            let block = &mut raw.functions[function].blocks[block];
            block.instructions.remove(instruction);
            block.instruction_spans.remove(instruction);
            block.instruction_scopes.remove(instruction);
        }
        verify_mir(raw, cancel)?
    };
    Ok(PassResult { module, statistics })
}

struct Work<'a> {
    used: usize,
    limit: usize,
    cancel: &'a CancellationToken,
}

impl Work<'_> {
    fn charge(&mut self, count: usize) -> Result<(), MirVerificationError> {
        let error = |kind| MirVerificationError {
            function: None,
            block: None,
            instruction: None,
            span: None,
            kind,
        };
        self.cancel
            .check()
            .map_err(|_| error(MirVerificationErrorKind::Cancelled))?;
        self.used = self.used.saturating_add(count);
        if self.used > self.limit {
            return Err(error(MirVerificationErrorKind::Limit {
                resource: "MIR optimization work",
                limit: self.limit,
            }));
        }
        Ok(())
    }
}
