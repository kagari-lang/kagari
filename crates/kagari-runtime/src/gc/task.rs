//! Terminal publication updates one sealed Task payload without a native borrow.
use crate::{
    error::RuntimeError,
    gc::{GcHeap, HeapObjectId},
    native::binding::NativeResult,
    resource::HeapCharge,
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
        self.ensure_no_storage_borrow()?;
        let objects = self
            .objects
            .try_borrow()
            .map_err(|_| RuntimeError::module_validation("Task storage is borrowed"))?;
        let Some(object) = self.native_ref(&objects, *id) else {
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
        self.ensure_no_storage_borrow()?;
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
        if let Ok(value) = &outcome {
            self.publish_value(*value)?;
        }
        let mut objects = self.objects_mut()?;
        let Some(object) = self.native_mut(&mut objects, id) else {
            return Err(RuntimeError::module_validation("Task payload"));
        };
        let payload = object.payload::<TaskPayload>()?;
        if payload.id != task || payload.outcome.is_some() {
            return Err(RuntimeError::module_validation("Task already completed"));
        }
        let charge = HeapCharge::new(1, TaskPayload::outcome_bytes(Some(&outcome)));
        let change = self
            .resources
            .prepare_cleanup_heap_change(object.charge, charge.logical_units)?;
        object.payload_mut::<TaskPayload>()?.outcome = Some(outcome);
        object.charge = charge;
        change.commit(charge);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Runtime, error::RuntimeErrorKind, native::storage::NativeStorage, task::store::Identity,
        test_support::layouts::allocation_owner,
    };
    use kagari_types::{scalar::BuiltinType, ty::Ty};

    #[test]
    fn completion_publishes_after_blackening_and_rejects_condemned_results_before_commit() {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let task = TaskId(Identity {
            owner: 1,
            slot: 0,
            generation: 0,
        });
        let allocate = || {
            let object = NativeStorage::task()
                .prepare_payload(
                    &runtime.gc,
                    &Ty::Builtin(BuiltinType::Unit),
                    TaskPayload {
                        id: task,
                        outcome: None,
                    },
                    &owner,
                )
                .unwrap();
            runtime.gc.alloc_native(object).unwrap()
        };
        let id = allocate();
        let result = runtime.gc.alloc_string("result".into()).unwrap();
        let dead = runtime.gc.alloc_string("condemned".into()).unwrap();
        runtime.gc.graph.lock().begin_cycle().unwrap();
        runtime.gc.graph.lock().finish_roots().unwrap();
        runtime.gc.publish_value(Value::GcHandle(id)).unwrap();
        runtime.gc.drain_graph_work(runtime.modules()).unwrap();
        runtime.gc.complete_task(id, task, Ok(result)).unwrap();
        runtime.gc.drain_graph_work(runtime.modules()).unwrap();
        runtime.gc.graph.lock().close_mark().unwrap();
        assert!(
            runtime
                .gc
                .graph_survivor(runtime.modules(), result.heap_identity().unwrap().graph())
                .unwrap()
        );
        let newborn = allocate();
        assert!(runtime.gc.complete_task(newborn, task, Ok(dead)).is_err());
        assert!(
            runtime
                .gc
                .task_snapshot(&Value::GcHandle(newborn))
                .unwrap()
                .1
                .is_none()
        );
        runtime.gc.complete_task(newborn, task, Ok(result)).unwrap();
        assert_eq!(
            runtime
                .gc
                .task_snapshot(&Value::GcHandle(newborn))
                .unwrap()
                .1
                .unwrap()
                .unwrap(),
            result
        );
        runtime.gc.graph.lock().finish_sweep().unwrap();
    }

    #[test]
    fn task_error_message_is_charged_even_when_publication_is_cleanup() {
        for state in ["running", "terminated", "quarantined"] {
            let mut runtime = Runtime::default();
            let owner = allocation_owner(&mut runtime);
            let _session = runtime.begin_execution(&owner, Default::default()).unwrap();
            let heap = runtime.gc();
            let task = TaskId(Identity {
                owner: 1,
                slot: 0,
                generation: 0,
            });
            let payload = TaskPayload {
                id: task,
                outcome: None,
            };
            let object = NativeStorage::task()
                .prepare_payload(heap, &Ty::Builtin(BuiltinType::Unit), payload, &owner)
                .unwrap();
            let id = heap.alloc_native(object).unwrap();
            let before = heap.stats();
            let mut message = String::with_capacity(4096);
            message.push_str("failure");
            let capacity = message.capacity();
            match state {
                "terminated" => {
                    heap.resource_limit("fixture termination");
                    assert!(heap.ensure_execution_allowed().is_err());
                }
                "quarantined" => {
                    heap.resources.quarantine("fixture fault");
                }
                _ => {}
            }
            heap.complete_task(
                id,
                task,
                Err(TaskFailure {
                    error: RuntimeError::new(RuntimeErrorKind::ScriptTrap, message),
                    source_task: task,
                    cancellation: None,
                }),
            )
            .unwrap();
            assert_eq!(heap.stats().current_heap_units, before.current_heap_units);
            assert_eq!(
                heap.stats().current_retained_bytes,
                before.current_retained_bytes + capacity as u128
            );
            let committed = heap.stats();
            assert!(
                heap.complete_task(
                    id,
                    task,
                    Err(TaskFailure {
                        error: RuntimeError::module_validation("duplicate"),
                        source_task: task,
                        cancellation: None,
                    })
                )
                .is_err()
            );
            assert_eq!(heap.stats(), committed);
            if state == "running" {
                runtime.collect_garbage().unwrap();
                assert_eq!(heap.stats().current_retained_bytes, 0);
            }
        }
    }
}
