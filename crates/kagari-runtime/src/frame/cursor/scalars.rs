//! Borrow code and scalar banks once while preserving each logical boundary.
use crate::{
    error::{RuntimeError, fault::EngineFault, reason::ErrorReason},
    frame::{
        cursor::{ExecutionCursor, kernel::RegionError},
        transfer::ReturnValue,
    },
    module::{
        ModuleStore,
        execution::{ExecutionFunction, managed::PreparedManagedOperation, operands::ScalarSlot},
    },
    session::SessionState,
};

use kagari_types::scalar::error::ScalarError;

pub(super) enum ScalarExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

/// Defer owned diagnostics until the closed segment has stopped executing.
#[derive(Debug, thiserror::Error)]
#[error("{}", self.reason())]
enum ScalarFailure {
    Trap(ScalarError),
    InvalidOperandSlot,
    InvalidJumpTarget,
    InconsistentBanks,
    UnexpectedFunctionEnd,
    InvalidBranchCondition,
}

impl ScalarFailure {
    fn reason(&self) -> ErrorReason {
        match self {
            Self::Trap(reason) => (*reason).into(),
            Self::InvalidOperandSlot => EngineFault::InvalidExecutionOperandSlot.into(),
            Self::InvalidJumpTarget => EngineFault::InvalidFrameJumpTarget.into(),
            Self::InconsistentBanks => EngineFault::InconsistentScalarBankLengths.into(),
            Self::UnexpectedFunctionEnd => EngineFault::VerifiedFunctionFellThrough.into(),
            Self::InvalidBranchCondition => EngineFault::VerifiedBranchConditionIsNotBool.into(),
        }
    }
}

/// The scalar segment selects its mode once at entry. Only bounded
/// execution carries writable step state; both modes use the same instruction loop.
trait InstructionSlice {
    fn exhausted(&self) -> bool;

    fn advance(&mut self);
}

struct Unbounded;

impl InstructionSlice for Unbounded {
    fn exhausted(&self) -> bool {
        false
    }

    fn advance(&mut self) {}
}

struct Bounded<'a>(&'a mut usize);

impl InstructionSlice for Bounded<'_> {
    fn exhausted(&self) -> bool {
        *self.0 == 0
    }

    fn advance(&mut self) {
        *self.0 = self.0.saturating_sub(1);
    }
}

struct ScalarCursor<'code, 'banks> {
    function: &'code ExecutionFunction,
    ip: &'banks mut usize,
    executing: &'banks mut Option<usize>,
    payloads: &'banks mut [u64],
    initialized: &'banks mut [bool],
    modules: &'code ModuleStore,
    session: &'code SessionState,
    automatic_collection: bool,
}

impl ExecutionCursor<'_, '_, '_, '_> {
    pub(super) fn execute_scalars(
        &mut self,
        remaining: &mut Option<usize>,
        collection_due: bool,
        first: bool,
    ) -> Result<ScalarExit, RegionError> {
        // Lend the two disjoint scalar slices once for the complete generated
        // loop. No GC/source lookup or managed capability accompanies this view.
        let (payloads, initialized) = self.values.scalar_banks().map_err(RegionError::storage)?;
        let mut cursor = ScalarCursor {
            function: self.function,
            ip: self.ip,
            executing: self.executing,
            payloads,
            initialized,
            modules: self.modules,
            session: self.session,
            automatic_collection: self.automatic_collection,
        };
        match remaining {
            Some(remaining) => cursor.execute_slice(&mut Bounded(remaining), collection_due, first),
            None => cursor.execute_slice(&mut Unbounded, collection_due, first),
        }
        .map_err(|failure| RuntimeError::new(failure.reason()).into())
    }
}

impl ScalarCursor<'_, '_> {
    #[inline(always)]
    pub(super) fn payload(&self, slot: ScalarSlot) -> Option<u64> {
        if !self.initialized.get(slot.index()).copied().unwrap_or(false) {
            return None;
        }
        self.payloads.get(slot.index()).copied()
    }

    pub(super) fn write_payload(&mut self, slot: ScalarSlot, value: u64) -> Option<()> {
        *self.payloads.get_mut(slot.index())? = value;
        *self.initialized.get_mut(slot.index())? = true;
        Some(())
    }

    fn poll_requested(&self, collection_due: bool, automatic_collection: bool) -> bool {
        self.session.options.cancellation.check().is_err()
            || self.session.observer_attached.get()
            || collection_due
            || (automatic_collection && self.modules.abandonment_pending())
    }

    fn jump(&self, target: usize) -> Result<usize, ScalarFailure> {
        if target >= self.function.instructions.len() {
            return Err(ScalarFailure::InvalidJumpTarget);
        }
        Ok(target)
    }
}

mod execute;
