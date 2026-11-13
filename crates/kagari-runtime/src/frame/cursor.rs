//! An admitted interpreter scope cannot escape the closed execution operation.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, reason::ErrorReason},
    execution::roots::ExecutionRoots,
    frame::{
        ExecutionFrame, FrameStack,
        cursor::kernel::{CursorExit, RegionError, RegionExit},
        values::operands::OperandWindow,
    },
    host::{HostBorrowTable, HostRegistry},
    module::{
        ModuleStore, execution::ExecutionFunction, linked_execution::LinkedFunction,
        managed::revision::RevisionState,
    },
    native::context::ScriptInvoker,
    resource::ResourceState,
    session::SessionState,
};
use kagari_bytecode::{module::CallableTarget, program::ModuleRef};
use kagari_gc::{error::GcError, object::Gc};

mod indices;
pub mod kernel;
mod objects;
mod primitives;
mod scalars;
#[cfg(test)]
mod tests;
mod transitions;

/// No frame, source or payload loan crosses a callback, collector step or cold
/// transition. Exact body state comes from the frame's admitted dependency.
struct ExecutionCursor<'code, 'access, 'source, 'gc> {
    function: &'code ExecutionFunction,
    links: Option<&'code LinkedFunction>,
    ip: &'code mut usize,
    executing: &'code mut Option<usize>,
    values: OperandWindow<'access, 'source, 'gc>,
    body: Gc<'gc, RevisionState>,
    member: ModuleRef,
    modules: &'code ModuleStore,
    session: &'code SessionState,
    host: &'code HostRegistry,
    host_borrows: &'code HostBorrowTable,
    resources: &'code ResourceState,
    collection_due: bool,
    automatic_collection: bool,
}

impl FrameStack {
    /// Execute prepared operations, ending all source/control loans before a
    /// transition can allocate or lend mutable Runtime services to Native code.
    pub fn execute_region(
        &self,
        runtime: &mut Runtime,
        remaining: &mut Option<usize>,
        invoke_script: ScriptInvoker,
    ) -> Result<RegionExit, RegionError> {
        match self.execute_admitted_region(runtime, remaining)? {
            CursorExit::Region(exit) => Ok(exit),
            CursorExit::Transition(transition) => {
                self.complete_transition(runtime, transition, invoke_script)
            }
        }
    }

    fn execute_admitted_region(
        &self,
        runtime: &mut Runtime,
        remaining: &mut Option<usize>,
    ) -> Result<CursorExit, RegionError> {
        self.validate_top(runtime)?;
        let frame_slots = {
            let frame = self.current(runtime)?;
            frame.validate_runtime(runtime)?;
            frame.slots()
        };
        let collection_due = runtime
            .memory
            .service_requested(runtime.modules.abandonment_pending());
        let automatic_collection = runtime.memory.automatic_collection_enabled();
        let Runtime {
            memory,
            resources,
            modules,
            host,
            host_borrows,
            ..
        } = runtime;
        let mut frames = resources.sessions.frames_mut(self.session).ok_or_else(|| {
            resources.quarantine(EngineFault::ExecutionSlotsBorrowedAcrossInstruction)
        })?;
        let frame = frames
            .last_mut()
            .expect("admitted frame unchanged without execution");
        frame.completed = None;
        let session = resources
            .sessions
            .get(self.session)
            .expect("admitted session");
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let body = slots.dependency(frame_slots.body)?;
                let ExecutionFrame {
                    loaded,
                    target,
                    links,
                    ip,
                    executing,
                    ..
                } = frame;
                if slots
                    .scope()
                    .read(body, |body| body.code().program_identity())
                    != loaded.program_identity()
                {
                    return Err(GcError::Representation);
                }
                let CallableTarget::Script(function) = *target else {
                    return Ok(Err(RuntimeError::from(
                        EngineFault::ScalarRegionRequiresScriptFrame,
                    )
                    .into()));
                };
                let Some(function) = loaded.execution().functions.get(function.index()) else {
                    return Ok(Err(RuntimeError::from(
                        EngineFault::InvalidExecutionFunction,
                    )
                    .into()));
                };
                let values = frame_slots.operands(slots)?;
                Ok(ExecutionCursor {
                    function,
                    links: links.as_deref(),
                    ip,
                    executing,
                    values,
                    body,
                    member: loaded.slot(),
                    modules,
                    session: &session,
                    host,
                    host_borrows,
                    resources,
                    collection_due,
                    automatic_collection,
                }
                .execute_region(remaining))
            })
        });
        if matches!(
            &result,
            Ok(Ok(CursorExit::Region(
                RegionExit::Slice | RegionExit::Safepoint | RegionExit::Return(_)
            )))
        ) {
            frame.completed = frame.executing;
        }
        drop(session);
        drop(frames);
        // Quarantine/termination and trace capture happen after every source,
        // frame and session loan has ended, including partial-effect failures.
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|error| error.admit(runtime))
    }
}

impl ExecutionCursor<'_, '_, '_, '_> {
    fn invalid(&self) -> RegionError {
        RuntimeError::from(EngineFault::InvalidExecutionOperandSlot).into()
    }
}

impl RegionError {
    fn admit(self, runtime: &Runtime) -> Self {
        match self {
            Self::Runtime(error) => {
                if let ErrorReason::EngineFault(reason) = error.reason() {
                    runtime.resources().quarantine(*reason).into()
                } else {
                    error.admit_execution_error(runtime.resources()).into()
                }
            }
            error => error,
        }
    }
}
