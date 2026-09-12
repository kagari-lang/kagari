use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use crate::{RuntimeError, RuntimeErrorKind};

#[derive(Debug, Clone, Copy, Default)]
enum Phase {
    #[default]
    Ready,
    Committing,
    Quarantined(&'static str),
}

/// Shared by execution and heap entry points in a single runtime.
#[derive(Debug, Default)]
pub(crate) struct ExecutionState(Cell<Phase>);

impl ExecutionState {
    pub(crate) fn ensure_allowed(&self) -> Result<(), RuntimeError> {
        match self.0.get() {
            Phase::Ready => Ok(()),
            Phase::Committing => {
                self.0.set(Phase::Quarantined(
                    "execution attempted during host path commit",
                ));
                self.ensure_allowed()
            }
            Phase::Quarantined(reason) => {
                Err(RuntimeError::new(RuntimeErrorKind::EngineFault, reason))
            }
        }
    }

    pub(crate) fn is_quarantined(&self) -> bool {
        matches!(self.0.get(), Phase::Quarantined(_))
    }

    pub(crate) fn commit(&self, commit: impl FnOnce()) -> Result<(), RuntimeError> {
        self.ensure_allowed()?;
        self.0.set(Phase::Committing);
        if catch_unwind(AssertUnwindSafe(commit)).is_err() {
            self.0.set(Phase::Quarantined("host path commit panicked"));
        }
        if matches!(self.0.get(), Phase::Committing) {
            self.0.set(Phase::Ready);
        }
        self.ensure_allowed()
    }
}
