//! Cold public slot mutation owns exclusive memory and a short control loan.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::{ExecutionRoots, Slots},
    frame::{ExecutionFrame, FrameStack},
    resource::ResourceState,
    value::{carriers::ScopedValue, view::ValueView},
    value_check::scoped::CheckError,
};
use kagari_bytecode::instruction::Slot;
use kagari_gc::error::GcError;

impl ExecutionFrame {
    /// Read a requested live binding through its admitted physical source. The
    /// observer owner authenticates this actual frame and owns any escaping root;
    /// source access cannot escape the caller's closed GC brand.
    pub(crate) fn read_observed_binding<'gc>(
        &self,
        slots: &mut Slots<'_, 'gc>,
        resources: &ResourceState,
        register: Slot,
    ) -> Result<ScopedValue<'gc>, CheckError> {
        let location = self.slot_location(resources, register, true)?;
        self.slots().read(slots, location).map_err(Into::into)
    }
}

enum SlotInput<'value, 'view, S> {
    Value(&'value ValueView<'view>),
    Slot(S),
}

enum SlotTransferFailure {
    Frame(GcError),
    Value(GcError),
}

impl FrameStack {
    /// Write the current frame's logical slot without acquiring an owning root.
    ///
    /// Runtime/source ownership, logical lifetime and the prepared destination
    /// domain are checked before commit. A failed write preserves the old slot
    /// and completion PC. The input must already be protected by its call owner;
    /// use [`Self::copy_slot`] to transfer another slot under the same operation.
    pub fn write_slot(
        &self,
        runtime: &mut Runtime,
        register: Slot,
        value: &ValueView<'_>,
    ) -> Result<(), RuntimeError> {
        self.change_slot(runtime, register, SlotInput::Value(value))
    }

    /// Copy the current frame's source slot before publishing its destination.
    /// Both logical lifetimes are admitted without an escaping intermediate view.
    pub fn copy_slot(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        source: Slot,
    ) -> Result<(), RuntimeError> {
        self.change_slot(runtime, destination, SlotInput::Slot(source))
    }

    fn change_slot(
        &self,
        runtime: &mut Runtime,
        destination: Slot,
        input: SlotInput<'_, '_, Slot>,
    ) -> Result<(), RuntimeError> {
        self.validate_top(runtime)?;
        // Cold admission may capture an error trace. Keep only a shared control
        // loan until owner/logical-lifetime errors have been decided.
        let (frame_slots, destination_location, input) = {
            let frame = self.current(runtime)?;
            frame.validate_runtime(runtime)?;
            let destination_location =
                frame.slot_location(runtime.resources(), destination, false)?;
            let input = match input {
                SlotInput::Slot(source) => {
                    SlotInput::Slot(frame.slot_location(runtime.resources(), source, true)?)
                }
                SlotInput::Value(value) => SlotInput::Value(value),
            };
            (frame.slots(), destination_location, input)
        };
        let Runtime {
            memory, resources, ..
        } = runtime;
        let mut frames = resources.sessions.frames_mut(self.session).ok_or_else(|| {
            resources.quarantine(EngineFault::FrameStackIsBorrowedAcrossExecution)
        })?;
        let frame = frames
            .last_mut()
            .expect("admitted frame unchanged without execution");
        // This is collection-free physical transfer. No cancellation/observer,
        // converter, payload callback or other executing service runs while the
        // short control loan and paired source scope overlap.
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| {
                    let value = match input {
                        SlotInput::Slot(source) => frame_slots
                            .read(slots, source)
                            .map_err(SlotTransferFailure::Frame)?,
                        SlotInput::Value(value) => {
                            value.get(slots).map_err(SlotTransferFailure::Value)?
                        }
                    };
                    frame_slots
                        .write(slots, destination_location, value)
                        .map_err(SlotTransferFailure::Frame)
                })())
            })
        });
        if matches!(result, Ok(Ok(()))) {
            frame.complete_slot_write(destination);
        }
        drop(frames);
        result
            .map_err(|error| runtime.managed_memory_error(error))?
            .map_err(|failure| match failure {
                SlotTransferFailure::Frame(
                    GcError::ForeignHeap | GcError::Index | GcError::Representation,
                ) => runtime
                    .resources()
                    .quarantine(EngineFault::InvalidFrameRegister),
                SlotTransferFailure::Value(
                    error @ (GcError::ForeignHeap
                    | GcError::InvalidRoot
                    | GcError::Index
                    | GcError::Representation),
                ) => RuntimeError::module_validation(error.to_string()),
                SlotTransferFailure::Frame(error) | SlotTransferFailure::Value(error) => {
                    runtime.managed_memory_error(error)
                }
            })
    }
}
