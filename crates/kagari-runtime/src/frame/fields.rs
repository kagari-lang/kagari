//! Cold field preparation ends control/source loans before Host conversion.
pub(super) mod access;
use crate::{
    Runtime,
    error::fault::EngineFault,
    execution::roots::ExecutionRoots,
    frame::{
        ExecutionFrame, FrameStack,
        cursor::kernel::{RegionError, RegionExit},
    },
    module::execution::fields::{FieldAccess, PreparedFieldOperation},
    value::place::ResultPlace,
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
    pub(super) fn execute_scoped_field(
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
        if let Some(access) = operation.reference {
            let (base, input) = runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let base = frame_slots.view(slots, operation.base)?;
                        let input = match operation.access {
                            FieldAccess::Read { .. } => None,
                            FieldAccess::Write { value } => Some(frame_slots.view(slots, value)?),
                        };
                        Ok((base, input))
                    })
                })
                .map_err(RegionError::storage)?;
            let view = runtime.scoped_view(&base)?;
            match operation.access {
                FieldAccess::Read { dst } => {
                    let published = runtime.read_scoped_field(
                        view,
                        &layout,
                        operation.slot as usize,
                        access,
                        ResultPlace::frame(&frame_slots, Some(&dst)),
                    )?;
                    published.admit_frame(&frame_slots, Some(&dst))?;
                }
                FieldAccess::Write { .. } => {
                    runtime.write_scoped_field(
                        view,
                        &layout,
                        operation.slot as usize,
                        &input.expect("prepared write input"),
                    )?;
                }
            }
        } else {
            let Runtime {
                memory,
                host,
                host_borrows,
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
                            host_borrows,
                            resources,
                        ))
                    })
                })
                .map_err(RegionError::storage)??;
        }
        self.complete_source_write(runtime);
        Ok(RegionExit::Safepoint)
    }
}
