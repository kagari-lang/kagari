//! Detached failure origins survive shared waits, report retirement and stripped debug records.
use super::Fixture;
use crate::{BytecodeArtifact, program::Compilation};
use kagari_bytecode::{instruction::BytecodeInstruction, module::CallableTarget};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_runtime::execution::owned::DriveResult;
use kagari_runtime::{
    error::RuntimeErrorKind,
    error_trace::asynchronous::{AsyncBoundary, MAX_ASYNC_BOUNDARIES},
    task::{CancellationCause, TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use std::{
    num::NonZeroUsize,
    slice,
    sync::{Arc, atomic::Ordering},
};

// Block-bodied waiters give Await an explicit tail-expression source span.
const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/embedding/async/failure-provenance.kgr");

fn fixture(stripped: bool) -> Fixture {
    Fixture::with_prepared_program(
        Default::default(),
        |_| {},
        |engine, _, _| {
            let artifact = engine
                .compile_to_artifact(
                    SourceFile::new("async-diagnostic.kgr", SOURCE),
                    Default::default(),
                )
                .unwrap();
            let artifact = if stripped {
                let definitions = DefinitionContext::new().unwrap();
                let mut artifact = artifact
                    .import_into(&definitions, &Default::default())
                    .unwrap()
                    .into_records();
                for module in &mut artifact.program.modules {
                    for function in &mut module.functions {
                        function.metadata.debug = Default::default();
                    }
                }
                let artifact = BytecodeArtifact::from_program(
                    artifact.program,
                    Default::default(),
                    &definitions,
                    &Default::default(),
                )
                .unwrap();
                DefinitionMetadata::checked(definitions.snapshot(), artifact, &Default::default())
                    .unwrap()
            } else {
                artifact
            };
            let decoded = BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap();
            Compilation::from_artifact(decoded, &Default::default(), &Default::default()).unwrap()
        },
    )
}

fn id(f: &mut Fixture, handle: &RootedValue) -> TaskId {
    f.runtime.task_id(handle).unwrap()
}

fn drive(f: &mut Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
    for _ in 0..1000 {
        f.runtime.collect_garbage().unwrap();
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
        let mut f = fixture(stripped);
        f.runtime.collect_garbage().unwrap();
        let baseline = f.runtime.gc_stats();

        let scope = f
            .runtime
            .create_dispatched_task_scope(
                &f.runtime.select_program(&f.module).unwrap(),
                Default::default(),
                Arc::new(|_| Ok(())),
            )
            .unwrap();
        let capability = (scope.capability()).clone();
        let producer = f.invoke("submit", slice::from_ref(&capability));
        let producer_id = id(&mut f, &producer);
        assert_eq!(drive(&mut f, &scope, producer_id), TaskDriveResult::Waiting);
        let left = f.invoke("observe", &[capability.clone(), producer.clone()]);
        let right = f.invoke("other", &[capability.clone(), producer.clone()]);
        for waiter in [&left, &right] {
            let waiter = id(&mut f, waiter);
            assert_eq!(drive(&mut f, &scope, waiter), TaskDriveResult::Waiting);
        }
        f.sent.lock().unwrap().pop().unwrap().complete(Ok(4));
        assert_eq!(
            drive(&mut f, &scope, producer_id),
            TaskDriveResult::Complete
        );
        let report = f
            .runtime
            .take_task_report(&scope, producer_id)
            .unwrap()
            .unwrap();
        let site = report.origin.site.as_ref().unwrap();
        assert_eq!(site.function_name, "submit");
        // The nested spawn inherits the enclosing tail expression's lowering span.
        // Executable origins retain it independently of optional debug records.
        let span = site.source_span.unwrap();
        assert_eq!(
            &SOURCE[span.start..span.end],
            "unwrap(scope.spawn(async || crash().await))"
        );
        assert_eq!(
            report.origin.factory.code_fingerprint,
            f.runtime
                .select_program(&f.module)
                .unwrap()
                .program_fingerprint()
        );
        let original = report.outcome.unwrap_err();
        assert_eq!(original.error.kind(), RuntimeErrorKind::ScriptTrap);
        let trace = original.error.trace().unwrap();
        assert!(trace.frames[0].function_name.contains("crash"), "{trace:?}");
        assert_eq!(trace.async_boundaries.len(), 1);
        let mut wait_sites = Vec::new();
        for (waiter, name) in [(&left, "observe"), (&right, "other")] {
            let waiter_id = id(&mut f, waiter);
            assert_eq!(drive(&mut f, &scope, waiter_id), TaskDriveResult::Complete);
            let failure = f
                .runtime
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
            let span = site.source_span.unwrap();
            assert_eq!(&SOURCE[span.start..span.end], "task.await");
            let CallableTarget::Script(function) = site.target else {
                panic!("script await")
            };
            let owner = f
                .runtime
                .select_program(&f.module)
                .unwrap()
                .member(site.module)
                .unwrap();
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
        let future = f.invoke("direct", slice::from_ref(&producer));
        let execution = f.start_future(&future);
        let DriveResult::Complete(Err(error)) = f
            .runtime
            .drive(&execution, NonZeroUsize::new(100).unwrap())
            .unwrap()
        else {
            panic!("cached failed Task propagates to the SDK")
        };

        assert_eq!(error.task_origin().unwrap().task, producer_id);
        assert_eq!(error.trace().unwrap().async_boundaries.len(), 2);
        drop((future, execution));
        // Cancellation before first drive has a spawn origin even without a body stack.
        let queued = f.invoke("submit", slice::from_ref(&capability));
        let queued_id = id(&mut f, &queued);
        f.runtime.cancel_task(&queued).unwrap();
        f.runtime.drain_cancelled_tasks().unwrap();
        let cancelled = f
            .runtime
            .take_task_report(&scope, queued_id)
            .unwrap()
            .unwrap()
            .outcome
            .unwrap_err();
        assert_eq!(cancelled.cancellation, Some(CancellationCause::Explicit));
        assert!(cancelled.error.trace().unwrap().frames.is_empty());
        let dependent = f.invoke("observe", &[capability.clone(), queued.clone()]);
        let dependent_id = id(&mut f, &dependent);
        assert_eq!(
            drive(&mut f, &scope, dependent_id),
            TaskDriveResult::Complete
        );
        let cancellation = f
            .runtime
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
            let next = f.invoke("observe", &[capability.clone(), tail.clone()]);
            let next_id = id(&mut f, &next);
            assert_eq!(drive(&mut f, &scope, next_id), TaskDriveResult::Complete);
            bounded = Some(
                f.runtime
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
        let hosted = f
            .runtime
            .spawn_task(&capability, &factory)
            .unwrap()
            .unwrap();
        let hosted_id = id(&mut f, &hosted);
        assert_eq!(drive(&mut f, &scope, hosted_id), TaskDriveResult::Complete);
        let host_report = f
            .runtime
            .take_task_report(&scope, hosted_id)
            .unwrap()
            .unwrap();
        assert!(host_report.origin.site.is_none());
        assert_eq!(
            host_report.origin.factory.code_fingerprint,
            f.runtime
                .select_program(&f.module)
                .unwrap()
                .program_fingerprint()
        );
        assert_eq!(host_report.outcome.unwrap().as_i32().unwrap(), 7);
        drop((
            factory, hosted, tail, left, right, queued, dependent, capability,
        ));
        scope.close();
        f.runtime.drain_cancelled_tasks().unwrap();
        drop(scope);
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.runtime.gc_stats().root_registrations,
            baseline.root_registrations,
            "retained diagnostics own no GC roots"
        );
        assert_eq!(
            f.runtime.gc_stats().objects,
            baseline.objects,
            "diagnostic sites own no script objects"
        );
        assert_eq!(
            f.runtime
                .modules()
                .retention_counts(f.runtime.select_program(&f.module).unwrap().key())
                .runtime_values,
            0,
            "diagnostics own no executable-version leases"
        );
    }
}
