use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    frame::{
        types::TypeEnvironment,
        values::{ExecutionValues, FrameSlots},
    },
    gc::NativeMutation,
    module::{
        ExecutableModule,
        execution::{
            calls::PreparedCall,
            layout::{AccessPhase, FrameLayout, Location},
        },
        linked_execution::LinkedFunction,
        state::initialization::StaticInitialization,
    },
    objects::invocation::MethodInvocation,
    resource::ResourceState,
    session::{ExecutionSession, store::SessionId},
    value::Value,
};

use kagari_abi::representation::ValueType;
use kagari_bytecode::{
    instruction::{BytecodeInstruction, Slot},
    module::{BytecodeFunction, CallableTarget},
    program::ModuleRef,
};
use std::{
    cell::RefMut,
    fmt,
    fmt::{Debug, Formatter},
    marker::PhantomData,
    rc::Rc,
    sync::Arc,
};

mod activation;
mod admission;
pub(crate) mod arguments;
mod calls;
mod closures;
pub mod cursor;
pub mod driver;
pub(crate) mod factory;
mod fields;
mod future;
mod interfaces;
mod layouts;
mod native;
mod owned;
mod returns;
mod shared;
mod stack;
mod statics;
pub mod transfer;
pub mod types;
pub(crate) mod values;
pub(crate) mod waiting;

/// An activation owns session lifetime and stack cleanup. Operations borrow the
/// control record and Runtime together; neither may escape this closed access.
#[must_use]
pub struct ExecutionStack<'runtime> {
    session: ExecutionSession<'runtime>,
    stack: Option<FrameStack>,
}

/// Checked coordinates for one execution suffix. The activation owns retirement;
/// this record contains no reference into Runtime or its frame/value storage.
pub struct FrameStack {
    _thread: PhantomData<Rc<()>>,
    session: SessionId,
    base: usize,
    id: u64,
    parked: bool,
}

#[derive(Clone, Copy)]
enum ReturnDestination {
    Slot(Option<Slot>),
    Prepared(Option<Location>),
}

#[derive(Debug)]
enum NativeEntryState {
    Script,
    Pending,
    Running,
    Complete,
}

pub struct ExecutionFrame {
    links: Option<Arc<LinkedFunction>>,
    environment: Option<TypeEnvironment>,
    loaded: ExecutableModule,
    target: CallableTarget,
    native_entry: NativeEntryState,
    ip: usize,
    executing: Option<usize>,
    // Published at closed-region/result boundaries, never charged per scalar instruction.
    completed: Option<usize>,
    native_origin: Option<usize>,
    slots: FrameSlots,
    slot_count: usize,
    slot_layout: Option<Arc<FrameLayout>>,
    return_to: ReturnDestination,
    invocation: Option<MethodInvocation>,
    mutations: Vec<(Value, NativeMutation)>,
    initializer: Option<StaticInitialization>,
}

impl Debug for ExecutionFrame {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutionFrame")
            .field("module", &self.loaded.key())
            .field("target", &self.target)
            .field("ip", &self.ip)
            .finish_non_exhaustive()
    }
}

enum FrameEntry {
    Call,
    Await,
    StaticInitializer(StaticInitialization),
}

struct FrameDispatch<'call> {
    prepared: Option<&'call PreparedCall>,
    entry: FrameEntry,
    invocation: Option<MethodInvocation>,
    environment: Option<TypeEnvironment>,
}

impl ExecutionFrame {
    pub(crate) fn release_values(&self, resources: &ResourceState) {
        drop(self.retire_values(resources));
    }

    /// Retain storage only for the closed retirement/publication transition.
    /// Failure quarantines execution but must not interrupt frame/depth cleanup.
    fn retire_values<'resources>(
        &self,
        resources: &'resources ResourceState,
    ) -> Option<RefMut<'resources, ExecutionValues>> {
        let mut values = resources.frame_values.try_borrow_mut().ok();
        if values
            .as_mut()
            .and_then(|values| values.release(self.slots))
            .is_none()
        {
            resources.quarantine(EngineFault::ExecutionSlotsRemainedBorrowedDuringFrameCleanup);
        }
        values
    }

    fn validate_runtime(&self, runtime: &Runtime) -> Result<(), RuntimeError> {
        if !self.slots.belongs_to(&runtime.gc) {
            return Err(RuntimeError::module_validation(
                "frame belongs to another runtime",
            ));
        }
        runtime.resources().ensure_execution_allowed()
    }

    /// Total prepared scalar and managed storage for the unified logical slots.
    pub fn physical_slot_count(&self) -> usize {
        self.slot_layout
            .as_ref()
            .map_or(0, |slot_layout| slot_layout.count)
    }

    /// Active scalar payload/initialization slots and managed Value slots,
    /// including visible bindings and native arguments. Prepared metadata is separate.
    pub fn storage_counts(&self, runtime: &Runtime) -> Result<(usize, usize), RuntimeError> {
        self.validate_runtime(runtime)?;
        let storage = runtime.resources().frame_values.try_borrow().map_err(|_| {
            runtime
                .resources()
                .quarantine(EngineFault::FrameStorageBorrowedDuringInspection)
        })?;
        let ranges = storage.ranges(self.slots).ok_or_else(|| {
            runtime
                .resources()
                .quarantine(EngineFault::ExpiredFrameStorage)
        })?;
        Ok((ranges.scalars.len(), ranges.managed.len()))
    }

    pub fn environment(&self) -> Option<TypeEnvironment> {
        self.environment.clone()
    }

    pub fn begin_native_mutation(
        &mut self,
        runtime: &Runtime,
        value: &Value,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        self.mutations
            .try_reserve(1)
            .map_err(|_| RuntimeError::from(TrapReason::MutationGuardAllocation))?;
        self.mutations
            .push((*value, runtime.gc.begin_native_mutation(value)?));
        Ok(())
    }

    pub fn end_native_mutation(&mut self, value: &Value) -> Result<(), RuntimeError> {
        if !self
            .mutations
            .last()
            .is_some_and(|(target, _)| target == value)
        {
            return Err(RuntimeError::from(TrapReason::MutationGuardMismatch));
        }
        self.mutations.pop();
        Ok(())
    }

    /// Fetch and advance without executing the instruction.
    ///
    /// Manual drivers must execute the fetched operation and apply its control
    /// flow before advancing again; slot guards do not validate skipped work.
    pub fn next_instruction(&mut self) -> Option<BytecodeInstruction> {
        let instruction = self.function()?.instructions.get(self.ip).cloned();
        if instruction.is_some() {
            self.completed = None;
            self.executing = Some(self.ip);
            self.ip += 1;
        }
        instruction
    }

    pub fn target(&self) -> CallableTarget {
        self.target
    }

    pub fn function(&self) -> Option<&BytecodeFunction> {
        match self.target {
            CallableTarget::Script(function) => {
                self.loaded.bytecode.functions.get(function.index())
            }
            CallableTarget::Native(_) => None,
        }
    }

    pub fn slot_type(&self, runtime: &Runtime, register: Slot) -> Result<ValueType, RuntimeError> {
        self.validate_runtime(runtime)?;
        self.function()
            .and_then(|function| function.metadata.slots.get(register.index()))
            .copied()
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidFrameRegisterType)
            })
    }

    pub fn module(&self) -> ModuleRef {
        self.loaded.slot()
    }

    pub fn loaded(&self) -> &ExecutableModule {
        &self.loaded
    }

    pub(crate) fn invocation(&self) -> Option<&MethodInvocation> {
        self.invocation.as_ref()
    }

    pub(crate) fn set_native_instruction(
        &mut self,
        runtime: &Runtime,
        offset: usize,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        let point = self
            .function()
            .and_then(|function| function.metadata.origins.get(offset))
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidNativeSemanticOrigin)
            })?;
        self.executing = Some(point.instruction);
        self.native_origin = Some(offset);
        Ok(())
    }

    /// Semantic origin emitted by native code, distinct from the executable PC.
    pub fn native_origin(&self) -> Option<usize> {
        self.native_origin
    }

    pub fn instruction_offset(&self) -> usize {
        self.executing.unwrap_or(self.ip)
    }

    /// Advance the observable program point only when this frame resumes.
    pub fn prepare_instruction(&mut self) {
        self.executing = None;
        self.completed = None;
        self.native_origin = None;
    }

    /// Select a final executable PC without manufacturing a reused logical value.
    /// Ordinary CFG edges and reexecution with intact inputs are admitted;
    /// other side entries cannot require a reused nonparameter value.
    pub fn jump_to(&mut self, runtime: &Runtime, offset: usize) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if self
            .function()
            .is_none_or(|function| offset >= function.instructions.len())
        {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::InvalidFrameJumpTarget));
        }
        let admitted = self
            .executing
            .and_then(|pc| self.function()?.instructions.get(pc))
            .is_some_and(|instruction| match instruction {
                BytecodeInstruction::Jump { target } => target.index() == offset,
                BytecodeInstruction::Branch {
                    then_target,
                    else_target,
                    ..
                }
                | BytecodeInstruction::PredicateBranch {
                    then_target,
                    else_target,
                    ..
                } => then_target.index() == offset || else_target.index() == offset,
                _ => false,
            });
        let side_entry =
            self.slot_layout
                .as_ref()
                .zip(self.function())
                .is_some_and(|(layout, function)| {
                    layout.admits_side_entry(
                        self.instruction_offset(),
                        offset,
                        &function.metadata.parameter_slots,
                    )
                });
        let reexecution = self.completed == Some(offset)
            && self
                .slot_layout
                .as_ref()
                .is_none_or(|layout| layout.admits_reexecution(offset));
        if !admitted && !reexecution && !side_entry {
            return Err(RuntimeError::module_validation(
                "frame jump would enter an uninitialized reused slot lifetime",
            ));
        }
        self.ip = offset;
        // Keep publication attached to the observed instruction until resume;
        // redirection cannot resurrect a consumed alias at that instruction.
        Ok(())
    }

    fn validate_slot_access(&self, slot: Slot, reading: bool) -> Result<(), RuntimeError> {
        let parameter = self
            .function()
            .is_some_and(|function| function.metadata.parameter_slots.contains(&slot));
        if self.slot_layout.as_ref().is_some_and(|layout| {
            !layout.admits_logical_access(
                slot.index(),
                self.instruction_offset(),
                reading,
                parameter,
                if self.completed == Some(self.instruction_offset()) {
                    AccessPhase::Complete
                } else if self.executing.is_some() {
                    AccessPhase::Executing
                } else {
                    AccessPhase::Pending
                },
            )
        }) {
            return Err(RuntimeError::module_validation(format!(
                "frame slot {} is outside its physical lifetime at instruction {} ({})",
                slot.index(),
                self.instruction_offset(),
                if reading { "read" } else { "write" },
            )));
        }
        Ok(())
    }

    /// Read a logical slot only while its reused physical location belongs to it.
    pub fn read_slot(&self, runtime: &Runtime, register: Slot) -> Result<Value, RuntimeError> {
        self.validate_runtime(runtime)?;
        if register.index() >= self.slot_count {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::InvalidFrameRegister));
        }
        self.validate_slot_access(register, true)?;
        self.slots
            .get(&runtime.gc, register.index())
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidFrameRegister)
            })
    }

    /// Write a logical slot without overwriting another slot's active lifetime.
    pub fn write_slot(
        &mut self,
        runtime: &Runtime,
        register: Slot,
        value: Value,
    ) -> Result<(), RuntimeError> {
        self.validate_runtime(runtime)?;
        if register.index() >= self.slot_count {
            return Err(runtime
                .resources()
                .quarantine(EngineFault::InvalidFrameRegister));
        }
        self.validate_slot_access(register, false)?;
        self.slots
            .set(&runtime.gc, register.index(), value)
            .ok_or_else(|| {
                runtime
                    .resources()
                    .quarantine(EngineFault::InvalidFrameRegister)
            })?;
        if self
            .executing
            .and_then(|pc| self.function()?.instructions.get(pc))
            .and_then(BytecodeInstruction::slot_output)
            == Some(register)
        {
            self.completed = self.executing;
        }
        Ok(())
    }
}

impl Runtime {
    /// Native backends publish their logical program point before a cancellation/GC safepoint.
    pub(crate) fn record_native_instruction(&self, offset: usize) -> Result<(), RuntimeError> {
        let session = self.resources().active_session().ok_or_else(|| {
            self.resources()
                .quarantine(EngineFault::NativeProgramPointWithoutAnExecutionSession)
        })?;
        let mut frames = self
            .resources()
            .sessions
            .frames_mut(session.id)
            .ok_or_else(|| {
                self.resources()
                    .quarantine(EngineFault::NativeProgramPointWhileFramesAreBorrowed)
            })?;
        let frame = frames.last_mut().ok_or_else(|| {
            self.resources()
                .quarantine(EngineFault::NativeProgramPointWithoutAnExecutionFrame)
        })?;
        frame.set_native_instruction(self, offset)
    }
}

#[cfg(test)]
mod execution_frames;
