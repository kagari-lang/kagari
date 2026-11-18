//! Cold field preparation ends control/source loans before opening the operand source.
pub(super) mod access;
use crate::{
    Runtime,
    error::fault::EngineFault,
    execution::roots::ExecutionRoots,
    frame::{
        ExecutionFrame, FrameStack,
        cursor::kernel::{RegionError, RegionExit},
    },
    module::execution::fields::PreparedFieldOperation,
};
use kagari_bytecode::module::CallableTarget;

impl ExecutionFrame {
    pub(super) fn prepared_field(&self, index: usize) -> Option<&PreparedFieldOperation> {
        let CallableTarget::Script(function) = self.target else {
            return None;
        };
        self.loaded
            .execution()
            .functions
            .get(function.index())?
            .fields
            .get(index)
    }
}

impl FrameStack {
    pub(super) fn execute_prepared_field(
        &self,
        runtime: &mut Runtime,
        index: usize,
    ) -> Result<RegionExit, RegionError> {
        let (operation, frame_slots) = {
            let frame = self.current(runtime)?;
            let operation = *frame.prepared_field(index).ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidPreparedFieldOrdinal)
            })?;
            (operation, frame.slots())
        };
        // A write's original input precedes fallible layout preparation and
        // receiver access, including its scalar tag and initialization state.
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let mut values = frame_slots.operands(slots)?;
                    Ok(access::admit_input(&mut values, &operation).map(|_| ()))
                })
            })
            .map_err(RegionError::storage)??;
        let layout = self
            .current(runtime)?
            .field_layout(runtime, operation.layout_site)?;
        let Runtime {
            memory,
            host,
            resources,
            ..
        } = runtime;
        memory
            .scope(resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let mut values = frame_slots.operands(slots)?;
                    Ok(access::execute(
                        &mut values,
                        &operation,
                        &layout,
                        host,
                        resources,
                    ))
                })
            })
            .map_err(RegionError::storage)??;
        self.complete_source_write(runtime);
        Ok(RegionExit::Safepoint)
    }
}
