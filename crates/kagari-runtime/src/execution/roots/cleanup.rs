//! Infallible source-owner cleanup quarantines retirement failures once.
use crate::{
    Runtime,
    error::fault::EngineFault,
    execution::roots::{ExecutionRoots, LocalMark},
};
use kagari_gc::{error::GcError, heap::Phase};

impl Runtime {
    pub(crate) fn retire_execution_locals(&mut self, mark: LocalMark) {
        match self.memory.stats().phase {
            Phase::Closed => return,
            Phase::Faulted => {
                if !self.resources.is_quarantined() {
                    self.resources
                        .quarantine(EngineFault::ExecutionStorageRetirementFailed(
                            GcError::Faulted,
                        ));
                }
                return;
            }
            _ => {}
        }
        if let Err(error) = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| slots.retire_locals(mark))
        }) && !self.resources().is_quarantined()
        {
            self.resources()
                .quarantine(EngineFault::ExecutionStorageRetirementFailed(error));
        }
    }
}
