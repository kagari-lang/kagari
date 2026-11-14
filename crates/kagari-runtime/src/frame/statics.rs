//! Static transitions use the admitted body's real shared cell and caller prefix.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    frame::{
        ExecutionFrame, FrameDispatch, FrameEntry, FrameStack, arguments::FrameArguments,
        driver::ReturnAction, instruction::Inputs, transfer::ReturnValue,
    },
    module::{
        managed::cell::StaticCellObject,
        state::{
            access::{self, StaticAccess},
            initialization::StaticCommit,
        },
    },
    native::conversion::arguments::ArgumentValues,
    resource::ResourceState,
};
use kagari_bytecode::{
    instruction::{BytecodeInstruction, ModuleSlot, Slot},
    module::CallableTarget,
};
use std::slice;

impl FrameStack {
    fn static_access(
        &self,
        runtime: &Runtime,
        slot: ModuleSlot,
    ) -> Result<StaticAccess, RuntimeError> {
        let frame = self.current(runtime)?;
        Ok(StaticAccess {
            owner: frame.loaded().clone(),
            body: frame
                .admitted_body()
                .map_err(|error| runtime.resources.quarantine(error))?,
            slot,
        })
    }

    /// False means the exact hidden initializer was admitted. Only this static
    /// access PC is retried; an already evaluated RHS/index is never replayed.
    pub fn ensure_static_ready(
        &self,
        runtime: &mut Runtime,
        slot: ModuleSlot,
    ) -> Result<bool, RuntimeError> {
        self.validate_top(runtime)?;
        let pc = {
            let frame = self.current(runtime)?;
            let pc = frame.executing.ok_or_else(|| {
                RuntimeError::module_validation("static access requires an executing instruction")
            })?;
            if !frame.function().and_then(|function| function.instructions.get(pc)).is_some_and(|instruction| {
                matches!(instruction, BytecodeInstruction::LoadModule { slot: operand, .. } | BytecodeInstruction::StoreModule { slot: operand, .. } if *operand == slot)
            }) {
                return Err(RuntimeError::module_validation("static access operand does not match current instruction"));
            }
            pc
        };
        let access = self.static_access(runtime, slot)?;
        let Some(attempt) = runtime.prepare_static_initialization(access)? else {
            return Ok(true);
        };
        let recipe = attempt.recipe().clone();
        let origin = attempt.origin();
        self.push_admitted_arguments(
            runtime,
            recipe.module,
            CallableTarget::Script(recipe.function),
            FrameArguments::pack(ArgumentValues::scalars(&[])),
            None,
            FrameDispatch {
                origin,
                metadata: None,
                prepared: None,
                entry: FrameEntry::StaticInitializer(attempt),
                invocation: None,
                environment: None,
            },
        )?;
        let mut frames = runtime
            .resources
            .sessions
            .frames_mut(self.session)
            .expect("initializer admission released frame control");
        let caller_index = frames.len() - 2;
        frames[caller_index].ip = pc;
        frames[caller_index].completed = None;
        Ok(false)
    }

    pub(crate) fn load_module_slot(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        slot: ModuleSlot,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare(self, runtime, &[], destination)?;
        let access = self.static_access(runtime, slot)?;
        runtime.require_frame_static_ready(&access)?;
        inputs.write(self, runtime, |cx| {
            let cell = access.resolve(cx.slots)?;
            let value = StaticCellObject::read(cx.slots.scope(), cell)?;
            access::check_ready(
                cx.slots.scope(),
                value,
                &access.owner,
                slot,
                cx.host,
                cx.borrows,
                cx.resources,
            )?;
            Ok(value)
        })
    }

    pub(crate) fn store_module_slot(
        &self,
        runtime: &mut Runtime,
        slot: ModuleSlot,
        source: Slot,
    ) -> Result<(), RuntimeError> {
        let inputs = Inputs::prepare_effect(self, runtime, slice::from_ref(&source))?;
        let access = self.static_access(runtime, slot)?;
        access::mutable(&access.owner, slot)?;
        runtime.require_frame_static_ready(&access)?;
        inputs.effect(self, runtime, |cx| {
            let cell = access.resolve(cx.slots)?;
            let previous = StaticCellObject::read(cx.slots.scope(), cell)?;
            access::check_ready(
                cx.slots.scope(),
                previous,
                &access.owner,
                slot,
                cx.host,
                cx.borrows,
                cx.resources,
            )?;
            let value = cx.field(0)?;
            access::check_input(
                cx.slots.scope(),
                value,
                &access.owner,
                slot,
                cx.host,
                cx.borrows,
                cx.resources,
            )?;
            cx.resources.poll_closed()?;
            Ok(StaticCellObject::replace(cx.slots.scope(), cell, value)?)
        })
    }

    pub(super) fn finish_static_return(
        &self,
        runtime: &mut Runtime,
        packet: ReturnValue,
    ) -> Result<ReturnAction, RuntimeError> {
        let (commit, action) = {
            let frames = runtime
                .resources
                .sessions
                .frames(self.session)
                .ok_or_else(|| {
                    runtime
                        .resources
                        .quarantine(EngineFault::InitializerReturnStackIsBorrowed)
                })?;
            let frame = frames.last().expect("checked initializer frame");
            let caller = frames
                .get(frames.len().checked_sub(2).ok_or_else(|| {
                    runtime
                        .resources
                        .quarantine(EngineFault::InitializerReturnLostItsCaller)
                })?)
                .filter(|_| frames.len() > self.base + 1)
                .ok_or_else(|| {
                    runtime
                        .resources
                        .quarantine(EngineFault::InitializerReturnLostItsCaller)
                })?;
            let commit = frame
                .initializer(&runtime.resources)
                .expect("checked initializer authority")
                .commit_target();
            // Preflight exit before committing the successful initializer. No
            // source growth or execution occurs between this check and retirement.
            runtime
                .memory
                .scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        frame
                            .storage
                            .as_ref()
                            .expect("live initializer storage")
                            .check_exit(slots)
                    })
                })
                .map_err(|error| runtime.managed_memory_error(error))?;
            (commit, caller.execution_action(runtime)?)
        };
        let result = self.with_result(runtime, packet, |runtime, value| {
            commit.commit(runtime, &value)
        });
        if let Err(error) = result {
            commit.fail(&error.to_string());
            runtime.flush_static_failures()?;
            return Err(error);
        }
        let mut frames = runtime
            .resources
            .sessions
            .frames_mut(self.session)
            .ok_or_else(|| {
                runtime
                    .resources
                    .quarantine(EngineFault::InitializerReturnStackIsBorrowed)
            })?;
        let retired = runtime.memory.scope(&runtime.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                frames
                    .last_mut()
                    .expect("checked initializer unchanged")
                    .storage
                    .take()
                    .expect("preflighted initializer storage")
                    .retire(slots)
            })
        });
        let frame = if retired.is_ok() { frames.pop() } else { None };
        drop(frames);
        retired.map_err(|error| runtime.managed_memory_error(error))?;
        runtime.resources.leave_call();
        if let Some(mut frame) = frame {
            frame.release_control(&runtime.resources);
        }
        Ok(ReturnAction::Continue(action))
    }

    /// Notify before cleanup. Actual cell mutation happens in a subsequent closed
    /// settlement, including for cancellation, observer failure and unwinding.
    pub fn fail_static_initializers(&self, runtime: &Runtime, cause: &str) {
        if let Some(frames) = runtime.resources.sessions.frames(self.session) {
            for frame in frames.iter().skip(self.base) {
                if let Some(initializer) = frame.initializer(&runtime.resources) {
                    initializer.fail(cause);
                }
            }
        }
    }
}

impl ExecutionFrame {
    pub(crate) fn pending_static_failure(&self, resources: &ResourceState) -> Option<StaticCommit> {
        self.initializer(resources)?.pending_failure()
    }
}
