use crate::{
    Runtime,
    error::{RuntimeError, fault::EngineFault, trap::TrapReason},
    task::{
        TaskFailure, TaskId,
        payload::{PreparedTaskFailure, TaskCompletion, TaskPayload},
        store::Identity,
    },
    test_support::layouts::allocation_owner,
};
use kagari_gc::error::GcError;

#[test]
fn task_error_message_is_charged_even_when_publication_is_cleanup() {
    for state in ["running", "terminated", "quarantined"] {
        let mut runtime = Runtime::default();
        let owner = allocation_owner(&mut runtime);
        let mut session = runtime.begin_execution(&owner, Default::default()).unwrap();
        let runtime = session.runtime_mut();
        runtime.collect_garbage().unwrap();
        let task = TaskId(Identity {
            owner: 1,
            slot: 0,
            generation: 0,
        });
        // This physical-cache test needs no fabricated Native header or nominal
        // type. Runtime admission/publication is sampled by task integration tests.
        let root = runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let task = TaskPayload::alloc(cx, task)?;
                cx.retain(task)
            })
            .unwrap();
        let before = runtime.gc_stats();
        let mut message = String::with_capacity(4096);
        message.push_str("failure");
        let capacity = message.capacity();
        match state {
            "terminated" => {
                runtime.resources().limit("fixture termination");
                assert!(runtime.resources().ensure_execution_allowed().is_err());
            }
            "quarantined" => {
                runtime
                    .resources()
                    .quarantine(EngineFault::HostInvariant("fixture fault"));
            }
            _ => {}
        }
        let prepared = PreparedTaskFailure::new(TaskFailure {
            error: RuntimeError::from(TrapReason::Provider(message.into())),
            source_task: task,
            cancellation: None,
        });
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let owner = cx.get(&root)?;
                TaskPayload::fail(cx, owner, task, prepared)
            })
            .unwrap();
        let after = runtime.gc_stats();
        assert_eq!(after.provider_units, before.provider_units);
        assert_eq!(after.indirect_bytes, before.indirect_bytes + capacity);
        let duplicate = PreparedTaskFailure::new(TaskFailure {
            error: RuntimeError::module_validation("duplicate"),
            source_task: task,
            cancellation: None,
        });
        assert_eq!(
            runtime.memory.scope(&runtime.resources, |cx, _| {
                let owner = cx.get(&root)?;
                TaskPayload::fail(cx, owner, task, duplicate)
            }),
            Err(GcError::Representation)
        );
        assert_eq!(runtime.gc_stats(), after);
        runtime
            .memory
            .scope(&runtime.resources, |cx, _| {
                let owner = cx.get(&root)?;
                assert_eq!(cx.read(owner, TaskPayload::id), task);
                match cx.read(owner, TaskPayload::completion) {
                    TaskCompletion::Failure(failure) => {
                        assert_eq!(failure.error.to_string(), "failure")
                    }
                    _ => panic!("terminal failure cache"),
                }
                Ok(())
            })
            .unwrap();
        drop(root);
        if state == "running" {
            runtime.collect_garbage().unwrap();
            assert_eq!(runtime.gc_stats().indirect_bytes, before.indirect_bytes);
        }
    }
}
