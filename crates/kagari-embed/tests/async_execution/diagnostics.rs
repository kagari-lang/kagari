//! Detached failure origins survive shared waits, report retirement and missing source maps.
use super::{Fixture, Value};
use kagari_bytecode::{instruction::BytecodeInstruction, module::CallableTarget};
use kagari_embed::{BytecodeArtifact, program::PreparedProgram, runtime::owned::DriveResult};
use kagari_runtime::{
    error::RuntimeErrorKind,
    error_trace::asynchronous::{AsyncBoundary, MAX_ASYNC_BOUNDARIES},
    gc::roots::RootedValue,
    task::{CancellationCause, TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
};
use kagari_source::source::SourceFile;
use std::{
    num::NonZeroUsize,
    slice,
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str = r#"
use test::async_sdk::request;
async fn crash() -> i32 { request(10).await; val zero=0; 1/zero }
fn unwrap(result: Result<Task<i32>, SpawnError>) -> Task<i32> {
    match result { Ok(task) => task, Err(_) => { val zero=0; val bad=1/zero; unwrap(result) } }
}
fn submit(scope: TaskScope) -> Task<i32> { unwrap(scope.spawn(async || crash().await)) }
fn observe(scope: TaskScope, task: Task<i32>) -> Task<i32> { unwrap(scope.spawn(async || task.await)) }
fn other(scope: TaskScope, task: Task<i32>) -> Task<i32> { unwrap(scope.spawn(async || task.await)) }
async fn direct(task: Task<i32>) -> i32 { task.await }
fn factory() -> fn()->Future<i32> { async || 7 }
"#;

fn fixture(stripped: bool) -> Fixture {
    let mut f = Fixture::new();
    let mut artifact = f
        .engine
        .compile_to_artifact(
            SourceFile::new("async-diagnostic.kgr", SOURCE),
            Default::default(),
        )
        .unwrap();
    if stripped {
        for module in &mut artifact.program.modules {
            for function in &mut module.functions {
                function.metadata.debug = Default::default();
            }
        }
        artifact = BytecodeArtifact::from_program(artifact.program, Default::default()).unwrap();
    }
    let decoded = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    f.program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    f.module = f
        .runtime
        .load_program(&f.program, Default::default())
        .unwrap();
    f
}

fn id(f: &Fixture, handle: &RootedValue) -> TaskId {
    f.runtime.runtime().task_id(&f.value(handle)).unwrap()
}

fn drive(f: &Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        f.runtime.runtime().collect_garbage().unwrap();
        let result = f
            .runtime
            .drive_task(scope, id, NonZeroUsize::new(1).unwrap())
            .unwrap();
        if result != TaskDriveResult::Runnable {
            return result;
        }
    }
    panic!("bounded diagnostic execution");
}

#[test]
fn async_failure_provenance_contract() {
    for stripped in [false, true] {
        let f = fixture(stripped);
        let runtime = f.runtime.runtime();
        let scope = f
            .runtime
            .create_task_scope(&f.module, &Default::default(), Arc::new(|_| Ok(())))
            .unwrap();
        let capability = f.value(scope.capability());
        let producer = f.invoke("submit", slice::from_ref(&capability));
        let producer_id = id(&f, &producer);
        assert_eq!(drive(&f, &scope, producer_id), TaskDriveResult::Waiting);
        let left = f.invoke("observe", &[capability, f.value(&producer)]);
        let right = f.invoke("other", &[capability, f.value(&producer)]);
        for waiter in [&left, &right] {
            assert_eq!(drive(&f, &scope, id(&f, waiter)), TaskDriveResult::Waiting);
        }
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(4));
        assert_eq!(drive(&f, &scope, producer_id), TaskDriveResult::Complete);
        let report = runtime
            .take_task_report(&scope, producer_id)
            .unwrap()
            .unwrap();
        let site = report.origin.site.as_ref().unwrap();
        assert_eq!(site.function_name, "submit");
        assert_eq!(site.source_span.is_none(), stripped);
        assert_eq!(
            report.origin.factory.code_fingerprint,
            f.module.program_fingerprint()
        );
        let original = report.outcome.unwrap_err();
        assert_eq!(original.error.kind(), RuntimeErrorKind::ScriptTrap);
        let trace = original.error.trace().unwrap();
        assert!(trace.frames[0].function_name.contains("crash"), "{trace:?}");
        assert_eq!(trace.async_boundaries.len(), 1);
        let mut wait_sites = Vec::new();
        for (waiter, name) in [(&left, "observe"), (&right, "other")] {
            let waiter_id = id(&f, waiter);
            assert_eq!(drive(&f, &scope, waiter_id), TaskDriveResult::Complete);
            let failure = runtime
                .take_task_report(&scope, waiter_id)
                .unwrap()
                .unwrap()
                .outcome
                .unwrap_err();
            assert_eq!(failure.source_task, producer_id);
            let observed = failure.error.trace().unwrap();
            assert_eq!(
                observed.frames, trace.frames,
                "keep the original failure stack"
            );
            assert_eq!(observed.async_boundaries.len(), 3);
            let AsyncBoundary::Await {
                task,
                site: Some(site),
            } = &observed.async_boundaries[1]
            else {
                panic!("await site: {observed:?}")
            };
            assert_eq!(*task, producer_id);
            assert_eq!(site.source_span.is_none(), stripped);
            let CallableTarget::Script(function) = site.target else {
                panic!("script await")
            };
            let owner = f.module.member(site.module).unwrap();
            assert!(matches!(
                owner.bytecode.functions[function.index()].instructions[site.instruction_offset],
                BytecodeInstruction::Await { .. }
            ));
            wait_sites.push((site.module, site.target, site.instruction_offset));
            assert!(
                matches!(&observed.async_boundaries[2], AsyncBoundary::Spawn { task, scope: owner, origin } if *task == waiter_id && *owner == scope.id() && origin.site.as_ref().unwrap().function_name == name)
            );
            assert!(observed.to_string().contains("awaited"));
        }
        assert_ne!(
            wait_sites[0], wait_sites[1],
            "independent waiters keep distinct portable sites"
        );
        assert_eq!(
            trace.async_boundaries.len(),
            1,
            "waiters cannot mutate the cached failure"
        );
        let future = f.invoke("direct", &[f.value(&producer)]);
        let execution = f.start_future(&future);
        let DriveResult::Complete(Err(error)) = f
            .runtime
            .drive(&execution, NonZeroUsize::new(100).unwrap())
            .unwrap()
        else {
            panic!("cached failed Task propagates to the SDK")
        };
        assert_eq!(error.task_origin().unwrap().task, producer_id);
        assert_eq!(error.error_trace().unwrap().async_boundaries.len(), 2);
        drop((future, execution));
        // Cancellation before first drive has a spawn origin even without a body stack.
        let queued = f.invoke("submit", slice::from_ref(&capability));
        let queued_id = id(&f, &queued);
        runtime.cancel_task(&f.value(&queued)).unwrap();
        runtime.drain_cancelled_tasks().unwrap();
        let cancelled = runtime
            .take_task_report(&scope, queued_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err();
        assert_eq!(cancelled.cancellation, Some(CancellationCause::Explicit));
        assert!(cancelled.error.trace().unwrap().frames.is_empty());
        let dependent = f.invoke("observe", &[capability, f.value(&queued)]);
        let dependent_id = id(&f, &dependent);
        assert_eq!(drive(&f, &scope, dependent_id), TaskDriveResult::Complete);
        let cancellation = runtime
            .take_task_report(&scope, dependent_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err();
        assert_eq!(cancellation.source_task, queued_id);
        assert_eq!(
            cancellation.error.trace().unwrap().async_boundaries.len(),
            3,
            "capturing the waiter's frame must retain the queued target's spawn origin"
        );
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        // Retired report slots cannot make a cached failure's causal chain unbounded.
        let mut tail = producer;
        let mut bounded = None;
        for _ in 0..MAX_ASYNC_BOUNDARIES {
            let next = f.invoke("observe", &[capability, f.value(&tail)]);
            let next_id = id(&f, &next);
            assert_eq!(drive(&f, &scope, next_id), TaskDriveResult::Complete);
            bounded = Some(
                runtime
                    .take_task_report(&scope, next_id)
                    .unwrap()
                    .unwrap()
                    .outcome
                    .unwrap_err(),
            );
            tail = next;
        }
        let bounded = bounded.unwrap();
        let bounded_trace = bounded.error.trace().unwrap();
        assert_eq!(bounded_trace.async_boundaries.len(), MAX_ASYNC_BOUNDARIES);
        assert_eq!(
            bounded_trace.omitted_async_boundaries,
            MAX_ASYNC_BOUNDARIES + 1
        );
        assert!(bounded_trace.incomplete);
        assert_eq!(bounded.source_task, producer_id);
        // Host admission has no script spawn site, but always retains factory identity.
        let factory = f.invoke("factory", &[]);
        let hosted = runtime
            .spawn_task(&capability, &f.value(&factory))
            .unwrap()
            .unwrap();
        let hosted_id = id(&f, &hosted);
        assert_eq!(drive(&f, &scope, hosted_id), TaskDriveResult::Complete);
        let host_report = runtime
            .take_task_report(&scope, hosted_id)
            .unwrap()
            .unwrap();
        assert!(host_report.origin.site.is_none());
        assert_eq!(
            host_report.origin.factory.code_fingerprint,
            f.module.program_fingerprint()
        );
        assert_eq!(f.value(&host_report.outcome.unwrap()), Value::I32(7));
        drop((factory, hosted, tail, left, right, queued, dependent));
        scope.close();
        runtime.drain_cancelled_tasks().unwrap();
        drop(scope);
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.gc().active_roots(),
            0,
            "retained diagnostics own no GC roots"
        );
        assert_eq!(
            runtime.gc().allocated_objects(),
            0,
            "diagnostic sites own no script objects"
        );
        assert_eq!(
            runtime
                .modules()
                .retention_counts(f.module.key())
                .runtime_values,
            0,
            "diagnostics own no executable-version leases"
        );
    }
}
