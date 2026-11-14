//! Cold allocation/preparation starts after closed-region loans have ended.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::roots::ExecutionRoots,
    frame::{
        FrameStack,
        calls::PreparedCallOutcome,
        cursor::kernel::{PreparedTransition, RegionError, RegionExit},
    },
    module::{execution::indices::IndexAccess, managed::revision::RevisionState},
    native::context::ScriptInvoker,
    value::{aggregates::tuples::TupleObject, carriers::ScopedValue, kind::ObjectKind},
};
use kagari_gc::error::GcError;

impl FrameStack {
    #[inline(never)]
    pub(super) fn complete_transition(
        &self,
        runtime: &mut Runtime,
        transition: PreparedTransition,
        invoke_script: ScriptInvoker,
    ) -> Result<RegionExit, RegionError> {
        let result = match transition {
            PreparedTransition::Call { index } => self
                .execute_prepared_call(runtime, index, invoke_script)
                .map(|outcome| match outcome {
                    PreparedCallOutcome::Frame(action) => RegionExit::Call(action),
                    PreparedCallOutcome::NativeComplete => RegionExit::Safepoint,
                })
                .map_err(RegionError::from),
            PreparedTransition::Upcast { index } => self
                .execute_interface_upcast(runtime, index)
                .map(|()| RegionExit::Safepoint)
                .map_err(RegionError::from),
            PreparedTransition::Field { index } => self.execute_scoped_field(runtime, index),
            PreparedTransition::TupleWrite { operation, index } => {
                self.replace_tuple(runtime, operation, index)
            }
            PreparedTransition::Constant { dst, constant } => {
                let (frame_slots, member) = {
                    let frame = self.current(runtime)?;
                    (frame.slots(), frame.loaded.slot())
                };
                let result = runtime.memory.scope(&runtime.resources, |cx, roots| {
                    ExecutionRoots::with(roots, cx, |slots| {
                        let body = slots.dependency(frame_slots.body)?;
                        let value = RevisionState::constant(slots.scope(), body, member, constant)?;
                        frame_slots.write(slots, dst, value)
                    })
                });
                result.map_err(|error| runtime.managed_memory_error(error))?;
                self.complete_source_write(runtime);
                Ok(RegionExit::Safepoint)
            }
        };
        result.map_err(|error| error.admit(runtime))
    }

    fn replace_tuple(
        &self,
        runtime: &mut Runtime,
        ordinal: usize,
        index: u64,
    ) -> Result<RegionExit, RegionError> {
        let (frame_slots, base, value) = {
            let frame = self.current(runtime)?;
            let operation = frame
                .prepared_index(ordinal)
                .ok_or_else(|| RuntimeError::from(EngineFault::InvalidTupleTransition))?;
            let IndexAccess::Write { value } = operation.access else {
                return Err(runtime
                    .resources()
                    .quarantine(EngineFault::InvalidTupleTransition)
                    .into());
            };
            (frame.slots(), operation.base, value)
        };
        let Runtime {
            memory, resources, ..
        } = runtime;
        let result = memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| {
                    let tuple = frame_slots
                        .read(slots, base)
                        .map_err(RegionError::storage)?;
                    let value = frame_slots
                        .read(slots, value)
                        .map_err(RegionError::storage)?;
                    if tuple.kind() != Some(ObjectKind::Tuple) {
                        return Err(RuntimeError::from(EngineFault::InvalidTupleTransition).into());
                    }
                    let object = slots
                        .scope()
                        .downcast::<TupleObject>(tuple.object().ok_or_else(|| {
                            RuntimeError::from(EngineFault::InvalidTupleTransition)
                        })?)
                        .map_err(RegionError::storage)?;
                    let count = slots.scope().read(object, TupleObject::len);
                    let position = usize::try_from(index)
                        .ok()
                        .filter(|&position| position < count)
                        .ok_or(RegionError::InvalidIndex(index))?;
                    let mut elements = Vec::new();
                    elements
                        .try_reserve_exact(count)
                        .map_err(|_| RegionError::storage(GcError::Allocation))?;
                    for element in 0..count {
                        if element.is_multiple_of(1024) {
                            resources.poll_closed()?;
                        }
                        let value = if element == position {
                            value
                        } else {
                            TupleObject::get(slots.scope(), object, element)
                                .map_err(RegionError::storage)?
                        };
                        elements.push(value);
                    }
                    resources.poll_closed()?;
                    let object = TupleObject::alloc(slots.scope(), &elements)
                        .map_err(RegionError::storage)?;
                    frame_slots
                        .write(slots, base, ScopedValue::managed(object))
                        .map_err(RegionError::storage)
                })())
            })
        });
        result.map_err(|error| runtime.managed_memory_error(error))??;
        self.complete_source_write(runtime);
        Ok(RegionExit::Safepoint)
    }

    pub(in crate::frame) fn complete_source_write(&self, runtime: &Runtime) {
        // No execution/callback intervenes between admitted private transition
        // and source publication. The completed write and PC commit are coupled;
        // do not introduce a fallible post-publication admission or poll here.
        let mut frames = runtime
            .resources()
            .sessions
            .frames_mut(self.session)
            .expect("closed source write released control loans");
        let frame = frames.last_mut().expect("admitted transition frame");
        frame.completed = frame.executing;
    }
}
