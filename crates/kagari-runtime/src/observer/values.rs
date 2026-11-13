//! Requested diagnostic values have explicit Host leases, never execution roots.
use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault},
    execution::{
        memory::ManagedMemory,
        roots::{ExecutionRoots, SessionId, Slots},
    },
    frame::ExecutionFrame,
    resource::ResourceState,
    value::{
        carriers::{RootedValue, ScopedValue},
        kind::ObjectKind,
    },
    value_check::scoped::CheckError,
};
use kagari_bytecode::instruction::Slot;
use kagari_gc::{
    error::GcError,
    object::{AnyObject, Gc},
};
use std::{cell::RefCell, collections::HashMap, ptr};

#[derive(Debug)]
pub(crate) struct ObservedValues {
    execution: SessionId,
    frames: Vec<HashMap<Slot, RootedValue>>,
}

impl ObservedValues {
    /// One closed scope authenticates current frame cells. A temporary same-brand
    /// identity map shares leases for aliases; it cannot outlive this capture or
    /// survive mutation, collection or a callback. Only opaque roots escape.
    pub(super) fn capture(
        memory: &mut ManagedMemory,
        resources: &ResourceState,
        execution: SessionId,
        frames: &[ExecutionFrame],
    ) -> Result<Self, CheckError> {
        memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                Ok((|| -> Result<Self, CheckError> {
                    let mut result = Self {
                        execution,
                        frames: Vec::new(),
                    };
                    result.frames.try_reserve_exact(frames.len())?;
                    let mut objects: HashMap<Gc<'_, AnyObject>, (ObjectKind, RootedValue)> =
                        HashMap::new();
                    for frame in frames {
                        resources.poll_closed()?;
                        let mut values = HashMap::new();
                        if let Some(function) = frame.function() {
                            let pc = frame.instruction_offset();
                            for (index, range) in
                                function.metadata.debug.local_live_ranges.iter().enumerate()
                            {
                                if index.is_multiple_of(1024) {
                                    resources.poll_closed()?;
                                }
                                if !(range.start <= pc && pc < range.end)
                                    || values.contains_key(&range.slot)
                                {
                                    continue;
                                }
                                let value =
                                    frame.read_observed_binding(slots, resources, range.slot)?;
                                let retained = retain_unique(slots, value, &mut objects)?;
                                values.try_reserve(1)?;
                                values.insert(range.slot, retained);
                            }
                        }
                        result.frames.push(values);
                    }
                    Ok(result)
                })())
            })
        })?
    }
}

fn retain_unique<'gc>(
    slots: &mut Slots<'_, 'gc>,
    value: ScopedValue<'gc>,
    objects: &mut HashMap<Gc<'gc, AnyObject>, (ObjectKind, RootedValue)>,
) -> Result<RootedValue, GcError> {
    let Some(object) = value.object() else {
        return value.retain(slots.scope());
    };
    if let Some((kind, retained)) = objects.get(&object) {
        return if Some(*kind) == value.kind() {
            Ok(retained.clone())
        } else {
            Err(GcError::Representation)
        };
    }
    objects.try_reserve(1).map_err(|_| GcError::Allocation)?;
    let retained = value.retain(slots.scope())?;
    objects.insert(
        object,
        (
            value.kind().ok_or(GcError::Representation)?,
            retained.clone(),
        ),
    );
    Ok(retained)
}

/// An inert release suffix: even a panicking observer cannot leave an active
/// capture installed. Escaping DebugBinding clones independently share its roots.
pub(super) struct CaptureGuard<'runtime> {
    cell: &'runtime RefCell<Option<ObservedValues>>,
    resources: &'runtime ResourceState,
}

impl<'runtime> CaptureGuard<'runtime> {
    pub(super) fn install(
        cell: &'runtime RefCell<Option<ObservedValues>>,
        resources: &'runtime ResourceState,
        values: ObservedValues,
    ) -> Result<Self, RuntimeError> {
        let mut slot = cell.try_borrow_mut().map_err(|_| {
            resources.quarantine(EngineFault::ExecutionObserverBorrowedAcrossExecution)
        })?;
        if slot.is_some() {
            return Err(resources.quarantine(EngineFault::ExecutionObserverBorrowedAcrossExecution));
        }
        *slot = Some(values);
        Ok(Self { cell, resources })
    }
}

impl Drop for CaptureGuard<'_> {
    fn drop(&mut self) {
        match self.cell.try_borrow_mut() {
            Ok(mut slot) => {
                let previous = slot.take();
                drop(slot);
                drop(previous);
            }
            Err(_) => {
                self.resources
                    .quarantine(EngineFault::ExecutionObserverBorrowedAcrossExecution);
            }
        }
    }
}

impl Runtime {
    /// Share a requested live debug binding's existing Host lease during readonly
    /// observation. This authenticates the actual active frame, not copied public
    /// metadata. Calls outside the requested callback or for dead slots fail.
    pub fn observed_binding_value(
        &self,
        frame: &ExecutionFrame,
        slot: Slot,
    ) -> Result<RootedValue, RuntimeError> {
        self.resources.ensure_synchronous_thread()?;
        self.resources.ensure_execution_allowed()?;
        let session = self.resources.active_session().ok_or_else(|| {
            RuntimeError::module_validation("binding observation requires an active session")
        })?;
        let frames = self
            .resources
            .sessions
            .frames(session.id)
            .ok_or_else(|| RuntimeError::module_validation("observed frame is borrowed"))?;
        let index = frames
            .iter()
            .position(|actual| ptr::eq(actual, frame))
            .ok_or_else(|| RuntimeError::module_validation("foreign observed frame"))?;
        let values = self
            .observed_values
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("binding observation is borrowed"))?;
        let values = values
            .as_ref()
            .filter(|values| values.execution == session.id)
            .ok_or_else(|| {
                RuntimeError::module_validation(
                    "binding values were not requested for this observation",
                )
            })?;
        values
            .frames
            .get(index)
            .and_then(|values| values.get(&slot))
            .cloned()
            .ok_or_else(|| {
                RuntimeError::module_validation("binding slot is not live in this observation")
            })
    }
}
