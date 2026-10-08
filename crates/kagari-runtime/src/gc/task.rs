//! Terminal publication updates one sealed Task payload without a native borrow.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId, storage::HeapObject},
    native::binding::NativeResult,
    task::{
        TaskFailure, TaskId,
        control::ScopeControl,
        payload::{ScopePayload, TaskPayload},
    },
    value::Value,
};
use std::sync::Arc;

impl GcHeap {
    pub(crate) fn scope_control(&self, value: &Value) -> NativeResult<Arc<ScopeControl>> {
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("TaskScope receiver"));
        };
        self.with_native::<ScopePayload, _>(*id, |payload| {
            if payload.id != payload.control.id {
                return Err(RuntimeError::module_validation("TaskScope identity"));
            }
            Ok(payload.control.clone())
        })
    }

    pub(crate) fn task_snapshot(
        &self,
        value: &Value,
    ) -> NativeResult<(TaskId, Option<Result<Value, TaskFailure>>)> {
        let Value::GcHandle(id) = value else {
            return Err(RuntimeError::module_validation("Task receiver"));
        };
        // Sealed control-plane reads remain available for failure reporting after
        // quarantine. They call no provider code and expose no storage borrow.
        self.ensure_no_native_borrow()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("Task storage is borrowed"))?;
        let Some(HeapObject::Native(object)) = self.readable_object(&objects, *id) else {
            return Err(RuntimeError::module_validation("Task payload"));
        };
        let payload = object.payload::<TaskPayload>()?;
        Ok((payload.id, payload.outcome.clone()))
    }

    pub(crate) fn complete_task(
        &self,
        id: HeapObjectId,
        task: TaskId,
        outcome: Result<Value, TaskFailure>,
    ) -> NativeResult<()> {
        self.ensure_no_native_borrow()?;
        // Error publication is cleanup of an already reserved, sealed slot. It
        // allocates no heap object and adds no GC edges, even after quarantine.
        if outcome.is_ok() {
            self.ensure_execution_allowed()?;
        }
        if let Ok(value) = &outcome
            && !self.valid_payload(value)
        {
            return Err(RuntimeError::module_validation("Task output"));
        }
        let mut objects = self.objects_mut()?;
        let Some(HeapObject::Native(object)) = self.object_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation("Task payload"));
        };
        let payload = object.payload_mut::<TaskPayload>()?;
        if payload.id != task || payload.outcome.is_some() {
            return Err(RuntimeError::module_validation("Task already completed"));
        }
        payload.outcome = Some(outcome);
        Ok(())
    }
}
