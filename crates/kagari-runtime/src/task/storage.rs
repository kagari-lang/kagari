//! Sealed task data access never lends payloads across notification or execution.
use crate::{
    Runtime,
    error::RuntimeError,
    execution::roots::{ExecutionRoots, ValueLocation},
    native::{binding::NativeResult, conversion::context::inputs::ValueInput},
    task::{
        TaskFailure, TaskId,
        control::ScopeStatus,
        payload::{PreparedTaskFailure, ScopePayload, TaskCompletion, TaskPayload},
    },
    value::{
        carriers::{RootedValue, ScopedValue},
        kind::ObjectKind,
        native::NativeObject,
    },
};
use kagari_gc::{
    access::Scope,
    error::GcError,
    heap::Phase,
    object::{Gc, GcObject},
};
use kagari_types::declaration::native::NativeStorageLayout;
use std::{sync::Arc, task::Poll};

pub(crate) enum TaskCachePublication {
    Stored,
    HeapUnavailable,
}

impl Runtime {
    pub(crate) fn scope_status(
        &mut self,
        value: ValueInput<'_, '_>,
    ) -> NativeResult<Arc<ScopeStatus>> {
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = value.get(slots)?;
                let scope =
                    payload::<ScopePayload>(slots.scope(), value, NativeStorageLayout::TaskScope)?;
                Ok(slots.scope().read(scope, |scope| scope.status.clone()))
            })
        });
        result.map_err(|error| self.task_storage_error(error))
    }

    pub(crate) fn task_state(&mut self, value: &RootedValue) -> NativeResult<(TaskId, bool)> {
        self.task_state_input(ValueInput::Root(value))
    }

    pub(crate) fn task_state_input(
        &mut self,
        value: ValueInput<'_, '_>,
    ) -> NativeResult<(TaskId, bool)> {
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = value.get(slots)?;
                let task = payload::<TaskPayload>(slots.scope(), value, NativeStorageLayout::Task)?;
                Ok(slots
                    .scope()
                    .read(task, |task| (task.id(), task.is_ready())))
            })
        });
        result.map_err(|error| self.task_storage_error(error))
    }

    pub(crate) fn task_state_at(&mut self, value: ValueLocation) -> NativeResult<(TaskId, bool)> {
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(value)?;
                let task = payload::<TaskPayload>(slots.scope(), value, NativeStorageLayout::Task)?;
                Ok(slots
                    .scope()
                    .read(task, |task| (task.id(), task.is_ready())))
            })
        });
        result.map_err(|error| self.task_storage_error(error))
    }

    /// Publish directly into a caller-owned protected result coordinate before
    /// the wait retires. No Host root or unrooted value pack bridges the boundary.
    pub(crate) fn poll_task_result(
        &mut self,
        value: ValueLocation,
        output: ValueLocation,
        expected: TaskId,
    ) -> NativeResult<Poll<Result<(), TaskFailure>>> {
        let result = self.memory.scope(&self.resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                let value = slots.get(value)?;
                let task = payload::<TaskPayload>(slots.scope(), value, NativeStorageLayout::Task)?;
                if slots.scope().read(task, TaskPayload::id) != expected {
                    return Err(GcError::Representation);
                }
                match slots.scope().read(task, TaskPayload::completion) {
                    TaskCompletion::Pending => Ok(Poll::Pending),
                    TaskCompletion::Failure(failure) => Ok(Poll::Ready(Err(failure))),
                    TaskCompletion::Success => {
                        let value = TaskPayload::get_result(slots.scope(), task)?;
                        slots.set(output, value)?;
                        Ok(Poll::Ready(Ok(())))
                    }
                }
            })
        });
        result.map_err(|error| self.task_storage_error(error))
    }

    /// Error-only publication is sealed cleanup after Runtime termination or
    /// quarantine. Faulted/closed GC never reopens; its host report owns the error.
    pub(crate) fn cache_task_result(
        &mut self,
        handle: &RootedValue,
        id: TaskId,
        outcome: Result<RootedValue, TaskFailure>,
    ) -> NativeResult<TaskCachePublication> {
        if matches!(self.memory.stats().phase, Phase::Faulted | Phase::Closed) {
            return match outcome {
                Err(_) => Ok(TaskCachePublication::HeapUnavailable),
                Ok(_) => Err(self.managed_memory_error(
                    if self.memory.stats().phase == Phase::Closed {
                        GcError::Closed
                    } else {
                        GcError::Faulted
                    },
                )),
            };
        }
        let result = match outcome {
            Ok(value) => {
                self.resources.ensure_execution_allowed()?;
                self.memory.scope(&self.resources, |cx, _| {
                    let handle = handle.get(cx)?;
                    let task = payload::<TaskPayload>(cx, handle, NativeStorageLayout::Task)?;
                    let value = value.get(cx)?;
                    TaskPayload::succeed(cx, task, id, value)
                })
            }
            Err(failure) => {
                let prepared = PreparedTaskFailure::new(failure);
                self.memory.scope(&self.resources, |cx, _| {
                    let handle = handle.get(cx)?;
                    let task = payload::<TaskPayload>(cx, handle, NativeStorageLayout::Task)?;
                    TaskPayload::fail(cx, task, id, prepared)
                })
            }
        };
        result
            .map(|()| TaskCachePublication::Stored)
            .map_err(|error| self.task_storage_error(error))
    }

    fn task_storage_error(&self, error: GcError) -> RuntimeError {
        match error {
            GcError::ForeignHeap | GcError::InvalidRoot | GcError::Representation => {
                RuntimeError::module_validation(error.to_string())
            }
            error => self.managed_memory_error(error),
        }
    }
}

fn payload<'gc, T: GcObject>(
    cx: &Scope<'gc>,
    value: ScopedValue<'gc>,
    role: NativeStorageLayout,
) -> Result<Gc<'gc, T>, GcError> {
    if value.kind() != Some(ObjectKind::Native) {
        return Err(GcError::Representation);
    }
    let owner = cx.downcast::<NativeObject>(value.object().ok_or(GcError::Representation)?)?;
    if cx.read(owner, |owner| owner.storage().layout()) != role {
        return Err(GcError::Representation);
    }
    let payload = NativeObject::payload(cx, owner);
    cx.downcast(payload)
}
