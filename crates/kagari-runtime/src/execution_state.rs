use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};

use crate::error::{RuntimeError, RuntimeErrorKind};

#[derive(Debug, Clone, Copy, Default)]
enum Phase {
    #[default]
    Ready,
    Committing,
    Collecting,
    Quarantined(&'static str),
}

/// Shared by execution and heap entry points in a single runtime.
#[derive(Debug, Default)]
pub(crate) struct ExecutionState(Cell<Phase>);

impl ExecutionState {
    pub(crate) fn quarantine(&self, reason: &'static str) -> RuntimeError {
        self.0.set(Phase::Quarantined(reason));
        RuntimeError::new(RuntimeErrorKind::EngineFault, reason)
    }

    pub(crate) fn ensure_allowed(&self) -> Result<(), RuntimeError> {
        match self.0.get() {
            Phase::Ready => Ok(()),
            Phase::Committing => {
                self.0.set(Phase::Quarantined(
                    "execution attempted during host path commit",
                ));
                self.ensure_allowed()
            }
            Phase::Collecting => {
                self.0.set(Phase::Quarantined(
                    "execution attempted during garbage collection",
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

    pub(crate) fn collect<T>(&self, collect: impl FnOnce() -> T) -> Result<T, RuntimeError> {
        self.ensure_allowed()?;
        self.0.set(Phase::Collecting);
        let result = catch_unwind(AssertUnwindSafe(collect))
            .map_err(|_| self.quarantine("garbage collection callback panicked"))?;
        if matches!(self.0.get(), Phase::Collecting) {
            self.0.set(Phase::Ready);
        }
        self.ensure_allowed()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_reentry_quarantines_even_when_the_callback_ignores_the_error() {
        let state = ExecutionState::default();
        let committed = Cell::new(false);
        let error = state
            .collect(|| {
                assert!(state.commit(|| committed.set(true)).is_err());
            })
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        assert!(!committed.get());
        assert!(state.is_quarantined());
        assert!(state.ensure_allowed().is_err());
    }

    #[test]
    fn collection_panic_is_contained_and_success_restores_execution() {
        let state = ExecutionState::default();
        assert_eq!(state.collect(|| 42).unwrap(), 42);
        assert!(state.ensure_allowed().is_ok());
        assert_eq!(
            state
                .collect(|| panic!("trace failure"))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(state.is_quarantined());
    }
}
