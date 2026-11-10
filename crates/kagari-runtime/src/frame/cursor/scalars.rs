//! Borrow code and scalar banks once while preserving each logical boundary.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    frame::{cursor::ExecutionCursor, transfer::ReturnValue},
    module::execution::{ScalarSlot, managed::PreparedManagedOperation},
};

pub(super) enum ScalarExit {
    Slice,
    Safepoint,
    Boundary,
    Return(ReturnValue),
    Managed(PreparedManagedOperation),
}

/// Defer owned diagnostics until the closed segment has stopped executing.
#[derive(Debug, thiserror::Error)]
#[error("{}", self.message())]
enum ScalarFailure {
    Trap(&'static str),
    InvalidOperandSlot,
    InvalidJumpTarget,
    InconsistentBanks,
    UnexpectedFunctionEnd,
    InvalidBranchCondition,
}

impl ScalarFailure {
    fn message(&self) -> &'static str {
        match self {
            Self::Trap(reason) => reason,
            Self::InvalidOperandSlot => "invalid execution operand slot",
            Self::InvalidJumpTarget => "invalid frame jump target",
            Self::InconsistentBanks => "inconsistent scalar bank lengths",
            Self::UnexpectedFunctionEnd => "verified function fell through",
            Self::InvalidBranchCondition => "verified branch condition is not bool",
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

impl<'code> ExecutionCursor<'code> {
    pub(super) fn execute_scalars(
        &mut self,
        remaining: &mut Option<usize>,
        collection_due: bool,
        first: bool,
    ) -> Result<ScalarExit, RuntimeError> {
        match remaining {
            Some(remaining) => self.execute_slice(&mut Bounded(remaining), collection_due, first),
            None => self.execute_slice(&mut Unbounded, collection_due, first),
        }
        .map_err(|failure| self.scalar_error(failure))
    }

    #[inline(always)]
    pub(super) fn payload(&self, slot: ScalarSlot) -> Option<u64> {
        if !self
            .values
            .initialized
            .get(slot.index())
            .copied()
            .unwrap_or(false)
        {
            return None;
        }
        self.values.payloads.get(slot.index()).copied()
    }

    pub(super) fn write_payload(&mut self, slot: ScalarSlot, value: u64) -> Option<()> {
        *self.values.payloads.get_mut(slot.index())? = value;
        *self.values.initialized.get_mut(slot.index())? = true;
        Some(())
    }

    fn poll_requested(&self, collection_due: bool, automatic_collection: bool) -> bool {
        self.session.options.cancellation.check().is_err()
            || self.session.observer_attached.get()
            || collection_due
            || (automatic_collection && self.runtime.modules.abandonment_pending())
    }

    fn jump(&self, target: usize) -> Result<usize, ScalarFailure> {
        if target >= self.function.instructions.len() {
            return Err(ScalarFailure::InvalidJumpTarget);
        }
        Ok(target)
    }

    #[cold]
    fn scalar_error(&self, failure: ScalarFailure) -> RuntimeError {
        match failure {
            ScalarFailure::Trap(reason) => RuntimeError::new(RuntimeErrorKind::ScriptTrap, reason),
            invariant => self.runtime.resources().quarantine(invariant.message()),
        }
    }
}

mod execute;
