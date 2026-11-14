//! Parked, queued and cold executions retain their checked generation at reload.
use super::Fixture;
use crate::{BytecodeArtifact, engine::EngineConfig, program::Compilation};
use kagari_runtime::execution::owned::DriveResult;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    error::RuntimeErrorKind,
    session::owned::OwnedExecution,
    task::{TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
    value::carriers::RootedValue,
};
use kagari_source::source::SourceFile;
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str =
    include_str!("../../../../../tests/fixtures/embedding/async/pinned-reload.kgr");

fn prepare(f: &Fixture, source: &str) -> Compilation {
    let artifact = f
        .engine
        .compile_to_artifact(
            SourceFile::new("async-script.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

fn task_boundary(f: &mut Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
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
    panic!("bounded Task continuation");
}

fn owned_boundary(f: &mut Fixture, execution: &OwnedExecution) -> DriveResult {
    for _ in 0..1000 {
        f.runtime.collect_garbage().unwrap();
        let result = f
            .runtime
            .drive(execution, NonZeroUsize::new(1).unwrap())
            .unwrap();
        if !matches!(result, DriveResult::Runnable) {
            return result;
        }
    }
    panic!("bounded owned continuation");
}

fn spawn(f: &mut Fixture, scope: &TaskScopeOwner, factory: &RootedValue) -> TaskId {
    let task = f
        .runtime
        .spawn_task(&(scope.capability()).clone(), factory)
        .unwrap()
        .unwrap();
    f.runtime.task_id(&task).unwrap()
}

fn reply(f: &Fixture, value: i32) {
    let endpoints = &mut *f.sent.lock().unwrap();
    assert_eq!(endpoints.len(), 1);
    endpoints.pop().unwrap().complete(Ok(value));
}

fn finish_task(f: &mut Fixture, scope: &TaskScopeOwner, id: TaskId, version: i32) {
    reply(f, 20);
    assert_eq!(task_boundary(f, scope, id), TaskDriveResult::Waiting);
    assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), 20 + version);
    reply(f, 30);
    assert_eq!(task_boundary(f, scope, id), TaskDriveResult::Complete);
    let result = f
        .runtime
        .take_task_report(scope, id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(result.as_i32().unwrap(), 55 + 2 * version);
}

fn reject_candidate_work(f: &mut Fixture, scope: &TaskScopeOwner, factory: &RootedValue) {
    let capability = (scope.capability()).clone();
    let factory_value = factory.clone();
    let ready = f.runtime.ready_tasks(scope).unwrap();
    for entry in ["cold", "native_cold", "spawn"] {
        let candidate = f
            .runtime
            .stage_reload_verified_program(&f.module, None, f.program.program().bytecode().clone())
            .unwrap();
        let mut session = f
            .runtime
            .begin_candidate_initialization(&candidate)
            .unwrap();
        if entry == "spawn" {
            assert_eq!(
                session
                    .runtime_mut()
                    .spawn_task(&capability, &factory_value)
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ExecutionPhaseViolation
            );
        } else {
            let options = session.runtime_mut().execution_options();
            let error = session
                .runtime_mut()
                .execute_module(candidate.module(), entry, options)
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    ref error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation
                ),
                "{error:?}"
            );
        }
        drop(session);
        drop(candidate);
        assert_eq!(f.runtime.ready_tasks(scope).unwrap(), ready);
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        assert!(!f.runtime.is_quarantined());
    }
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn string_cursor_retains_dynamic_source_across_native_waits() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut f = Fixture::configured_source(
        include_str!("../../../../../tests/fixtures/embedding/async/string-cursor-waits.kgr"),
        config,
    );
    let cursor = f.invoke("cursor", &[]);
    let future = f.invoke("traverse", slice::from_ref(&cursor));
    let owner = f.start_future(&future);
    drop(cursor);
    drop(future);
    // Inputs 0 and 1 complete immediately in the shared native fixture.
    for input in [10, 20, 30] {
        assert!(matches!(
            owned_boundary(&mut f, &owner),
            DriveResult::Waiting
        ));
        assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), input);
        f.runtime.collect_garbage().unwrap();
        assert_eq!(
            f.sent.lock().unwrap().pop().unwrap().complete(Ok(14)),
            CompletionStatus::Accepted
        );
    }
    let DriveResult::Complete(result) = owned_boundary(&mut f, &owner) else {
        panic!("String cursor must exhaust after three scalars")
    };
    let result = result.unwrap();
    assert_eq!(result.as_i32().unwrap(), 42);
    assert_eq!(*f.inputs.lock().unwrap(), [10, 20, 30]);
    drop(result);
    drop(owner);
    f.runtime.collect_garbage().unwrap();
    assert_eq!(f.runtime.gc_stats().root_registrations, 0);
    assert_eq!(f.runtime.resources().counters().current_call_depth, 0);
    assert!(!f.runtime.is_quarantined());
}
*/

#[test]
fn lifecycle_reload_contract() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut f = Fixture::configured_source(SOURCE, config);
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
    let old_key = f.runtime.select_program(&f.module).unwrap().key();
    let factory = f.invoke("factory", &[]);
    let future = f.invoke("cold", &[]);
    let captured = f.invoke("captured", &[]);
    let waiting = spawn(&mut f, &scope, &factory);
    let queued = spawn(&mut f, &scope, &captured);
    drop(captured);
    assert_eq!(
        task_boundary(&mut f, &scope, waiting),
        TaskDriveResult::Waiting
    );
    reject_candidate_work(&mut f, &scope, &factory);
    let original_version = "pub fn version() -> i32 {\n    1\n}";
    assert_eq!(SOURCE.matches(original_version).count(), 1);
    // Both publications happen while the nested `first` Future is waiting.
    // Its parent must refresh too: after child return it calls `version` again.
    for version in [100, 200] {
        let replacement = prepare(
            &f,
            &SOURCE.replace(
                original_version,
                &format!("pub fn version() -> i32 {{\n    {version}\n}}"),
            ),
        );
        f.runtime
            .program(&f.module)
            .map_err(ReloadValidationError::Runtime)
            .and_then(|base| base.prepare_replacement(replacement.program(), &Default::default()))
            .and_then(|candidate| {
                f.runtime
                    .replace_program(&f.module, &candidate, Default::default())
            })
            .unwrap();
        assert_ne!(f.runtime.select_program(&f.module).unwrap().key(), old_key);
        f.program = replacement;
        assert_eq!(f.invoke("version", &[]).as_i32().unwrap(), version);
    }
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "reload must not drive waiting or queued work"
    );
    assert!(f.runtime.modules().loaded(old_key).is_some());
    finish_task(&mut f, &scope, waiting, 200);
    // A Future captured before publication remains cold until its queued factory runs.
    assert_eq!(
        task_boundary(&mut f, &scope, queued),
        TaskDriveResult::Waiting
    );
    finish_task(&mut f, &scope, queued, 200);
    // The retained closure keeps its body; its ordinary factory call selects current code.
    let retained = spawn(&mut f, &scope, &factory);
    drop(factory);
    assert_eq!(
        task_boundary(&mut f, &scope, retained),
        TaskDriveResult::Waiting
    );
    finish_task(&mut f, &scope, retained, 200);
    let execution = f.start_future(&future);
    drop(future);
    assert!(matches!(
        owned_boundary(&mut f, &execution),
        DriveResult::Waiting
    ));
    reply(&f, 20);
    assert!(matches!(
        owned_boundary(&mut f, &execution),
        DriveResult::Waiting
    ));
    assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), 220);
    reply(&f, 30);
    let DriveResult::Complete(result) = owned_boundary(&mut f, &execution) else {
        panic!("completed retained Future")
    };
    assert_eq!(result.unwrap().as_i32().unwrap(), 455);
    drop(execution);
    let fresh_factory = f.invoke("factory", &[]);
    let fresh = spawn(&mut f, &scope, &fresh_factory);
    drop(fresh_factory);
    assert_eq!(
        task_boundary(&mut f, &scope, fresh),
        TaskDriveResult::Waiting
    );
    finish_task(&mut f, &scope, fresh, 200);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    scope.close();
    f.runtime.drain_cancelled_tasks().unwrap();
    drop(scope);
    f.runtime.collect_garbage().unwrap();
    assert_eq!(
        f.runtime.modules().retention_counts(old_key).runtime_values,
        0
    );
    assert_eq!(
        f.runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(f.runtime.gc_stats().objects, baseline.objects);
    assert!(f.runtime.modules().loaded(old_key).is_none());
}
