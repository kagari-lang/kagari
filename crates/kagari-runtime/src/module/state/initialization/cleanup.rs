//! Materialize detached failure while its actual caller source still protects cells.
use crate::{
    Runtime,
    error::RuntimeError,
    error::fault::EngineFault,
    execution::{memory::ManagedMemory, roots::ExecutionRoots},
    module::{
        managed::cell::{StaticCellObject, StaticPhase},
        state::initialization::StaticCommit,
    },
    resource::ResourceState,
};
use kagari_gc::error::GcError;
use std::sync::Arc;

impl StaticCommit {
    pub(crate) fn settle(
        &self,
        memory: &mut ManagedMemory,
        resources: &ResourceState,
    ) -> Result<(), GcError> {
        let Some(failure) = self.attempt.failure() else {
            return Ok(());
        };
        memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let cell = self.access.resolve(slots)?;
                if slots.scope().read(cell, StaticCellObject::phase) == StaticPhase::Failed {
                    return Ok(());
                }
                if !slots.scope().read(cell, |cell| {
                    cell.attempt()
                        .is_some_and(|actual| Arc::ptr_eq(actual, &self.attempt))
                }) {
                    return Err(GcError::Storage("static failure lost attempt authority"));
                }
                StaticCellObject::fail(slots.scope(), cell, failure.try_copy()?)
            })
        })
    }
}

impl Runtime {
    /// The shared signal does not own GC data or alter tracing. Only actual
    /// frame source coordinates can materialize a failure before collection.
    pub(crate) fn flush_static_failures(&mut self) -> Result<(), RuntimeError> {
        if !self.resources.take_static_failures() {
            return Ok(());
        }
        let result = (|| {
            let pending = self
                .resources
                .sessions
                .inspect_frame_stacks(|stacks| {
                    let mut pending = Vec::new();
                    for frame in stacks.iter().flatten() {
                        if let Some(failure) = frame.pending_static_failure() {
                            pending.try_reserve(1)?;
                            pending.push(failure);
                        }
                    }
                    Ok::<_, GcError>(pending)
                })
                .ok_or_else(|| {
                    self.resources
                        .quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
                })?
                .map_err(|error| self.managed_memory_error(error))?;
            for failure in pending {
                failure
                    .settle(&mut self.memory, &self.resources)
                    .map_err(|error| self.managed_memory_error(error))?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.resources.restore_static_failures();
        }
        result
    }
}
