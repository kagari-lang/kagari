//! An admitted interpreter scope cannot escape the closed execution operation.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, reason::ErrorReason},
    execution::roots::ExecutionRoots,
    frame::{
        ExecutionFrame, FrameStack,
        cursor::{
            calls::ClosedFrames,
            kernel::{CursorExit, PreparedTransition, RegionError, RegionExit},
        },
        values::{FrameAccessError, operands::OperandWindow},
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

mod calls;
mod indices;
pub mod kernel;
mod objects;
mod scalars;
mod tail;
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
    tail_elided: &'code mut bool,
    tail_allowed: bool,
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
    /// Execute prepared operations and closed scalar leaf transitions. End all
    /// source/control loans before collection, semantic adaptation or Native code.
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
        // Admit this closed interval once. These control loans exclude changes
        // to its session/scope/frame identities until paired source access ends.
        // Public frame access and the next post-callback region admit separately.
        let resources = &runtime.resources;
        let session = resources.sessions.get(self.session).ok_or_else(|| {
            RuntimeError::module_validation("execution stack belongs to another runtime")
        })?;
        resources.ensure_execution_allowed()?;
        if resources.active_session_id() != Some(self.session) {
            return Err(resources
                .quarantine(EngineFault::ExecutionUsedASuspendedSession)
                .into());
        }
        if session.frame_scopes.borrow().last() != Some(&self.id) {
            return Err(resources
                .quarantine(EngineFault::ExecutionUsedASuspendedFrameScope)
                .into());
        }
        let collection_due = runtime
            .memory
            .service_requested(runtime.modules.abandonment_pending());
        let automatic_collection = runtime.memory.automatic_collection_enabled();
        let Runtime {
            memory,
            modules,
            host,
            host_borrows,
            ..
        } = runtime;
        let mut frames = resources.sessions.frames_mut(self.session).ok_or_else(|| {
            resources.quarantine(EngineFault::ExecutionSlotsBorrowedAcrossInstruction)
        })?;
        if frames.len() <= self.base {
            return Err(resources
                .quarantine(EngineFault::MissingExecutionFrame)
                .into());
        }
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                loop {
                    let frame = frames.last_mut().expect("admitted execution frame");
                    let Some(storage) = &frame.storage else {
                        return Ok(Err(RuntimeError::module_validation(
                            "frame belongs to another runtime",
                        )
                        .into()));
                    };
                    let frame_slots = storage.slots();
                    let (body, mut values) = match frame_slots.execution_operands(slots) {
                        Ok(admitted) => admitted,
                        Err(FrameAccessError::Banks) => {
                            return Ok(Err(RuntimeError::module_validation(
                                "frame belongs to another runtime",
                            )
                            .into()));
                        }
                        Err(FrameAccessError::Storage(error)) => return Err(error),
                    };
                    frame.completed = None;
                    let tail_allowed = frame.environment.is_none()
                        && frame.uncommon.is_none()
                        && frame.native_origin.is_none()
                        && !resources.native_stack_active()
                        && !session.observer_attached.get();
                    let ExecutionFrame {
                        loaded,
                        target,
                        links,
                        ip,
                        executing,
                        tail_elided,
                        ..
                    } = frame;
                    if values
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
                    let result = ExecutionCursor {
                        function,
                        links: links.as_deref(),
                        ip,
                        executing,
                        tail_elided,
                        tail_allowed,
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
                    .execute_region(remaining);
                    if matches!(
                        &result,
                        Ok(CursorExit::Region(
                            RegionExit::Slice | RegionExit::Safepoint | RegionExit::Return(_)
                        ))
                    ) {
                        frame.completed = frame.executing;
                    }
                    let mut closed = ClosedFrames {
                        frames: &mut frames,
                        slots,
                        session: &session,
                        resources,
                        modules,
                        base: self.base,
                    };
                    let continued = match &result {
                        Ok(CursorExit::Transition(PreparedTransition::Call { index })) => {
                            closed.try_call(*index)
                        }
                        Ok(CursorExit::Region(RegionExit::Return(packet))) => {
                            closed.try_return(packet)
                        }
                        _ => return Ok(result),
                    };
                    match continued {
                        Ok(false) => return Ok(result),
                        Err(error) => return Ok(Err(error)),
                        Ok(true) => {}
                    }
                    // Match the outer driver's order: poll before slice parking,
                    // then prepare the next PC before collection/observation. A
                    // cancellation therefore still sees the completed caller Call.
                    if let Err(error) = resources.poll_closed() {
                        return Ok(Err(error.into()));
                    }
                    if *remaining == Some(0) {
                        return Ok(Ok(CursorExit::Region(RegionExit::Slice)));
                    }
                    frames
                        .last_mut()
                        .expect("continued script frame")
                        .prepare_instruction();
                    if collection_due || (automatic_collection && modules.abandonment_pending()) {
                        return Ok(Ok(CursorExit::Region(RegionExit::Safepoint)));
                    }
                }
            })
        });
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
