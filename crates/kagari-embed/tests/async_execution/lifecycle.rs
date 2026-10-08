//! Parked, queued and cold executions retain their checked generation at reload.
use super::{Fixture, Value};
use kagari_embed::{
    BytecodeArtifact,
    engine::EngineConfig,
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
    runtime::owned::DriveResult,
};
use kagari_runtime::{
    error::RuntimeErrorKind,
    gc::roots::RootedValue,
    session::owned::OwnedExecution,
    task::{TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
};
use kagari_source::source::SourceFile;
use std::{
    num::NonZeroUsize,
    sync::{Arc, atomic::Ordering},
};

const SOURCE: &str = r#"
use test::async_sdk::request;
fn version() -> i32 { 1 }
trait Read { fn read(self) -> i32; }
struct Saved { val value: i32 }
impl Read for Saved { fn read(self) -> i32 { self.value + version() } }
async fn finish<T: Read>(saved: T) -> i32 {
    val first = request(10).await;
    val second = request(first + version()).await;
    saved.read() + first + second + version()
}
fn factory() -> fn() -> Future<i32> { val saved = Saved { value: 5 }; || finish(saved) }
fn cold() -> Future<i32> { finish(Saved { value: 5 }) }
fn native_cold() -> Future<i32> { request(10) }
fn captured() -> fn() -> Future<i32> { val future = cold(); || future }
"#;

fn prepare(f: &Fixture, source: &str) -> PreparedProgram {
    let artifact = f
        .engine
        .compile_to_artifact(
            SourceFile::new("async-script.kgr", source),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

fn task_boundary(f: &Fixture, scope: &TaskScopeOwner, id: TaskId) -> TaskDriveResult {
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
    panic!("bounded Task continuation");
}

fn owned_boundary(f: &Fixture, execution: &OwnedExecution) -> DriveResult {
    for _ in 0..1000 {
        f.runtime.runtime().collect_garbage().unwrap();
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

fn spawn(f: &Fixture, scope: &TaskScopeOwner, factory: &RootedValue) -> TaskId {
    let task = f
        .runtime
        .runtime()
        .spawn_task(&f.value(scope.capability()), &f.value(factory))
        .unwrap()
        .unwrap();
    f.runtime.runtime().task_id(&f.value(&task)).unwrap()
}

fn reply(f: &Fixture, value: i32) {
    let endpoints = &mut *f.sent.lock().unwrap();
    assert_eq!(endpoints.len(), 1);
    endpoints.pop().unwrap().complete(Ok(value));
}

fn finish_task(f: &Fixture, scope: &TaskScopeOwner, id: TaskId, version: i32) {
    reply(f, 20);
    assert_eq!(task_boundary(f, scope, id), TaskDriveResult::Waiting);
    assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), 20 + version);
    reply(f, 30);
    assert_eq!(task_boundary(f, scope, id), TaskDriveResult::Complete);
    let result = f
        .runtime
        .runtime()
        .take_task_report(scope, id)
        .unwrap()
        .unwrap()
        .outcome
        .unwrap();
    assert_eq!(f.value(&result), Value::I32(55 + 2 * version));
}

fn reject_candidate_work(f: &Fixture, scope: &TaskScopeOwner, factory: &RootedValue) {
    let runtime = f.runtime.runtime();
    let ready = runtime.ready_tasks(scope).unwrap();
    for entry in ["cold", "native_cold", "spawn"] {
        let candidate = runtime
            .stage_reload_verified_program(&f.module, &f.module.name, f.program.bytecode().clone())
            .unwrap();
        let session = runtime.begin_candidate_initialization(&candidate).unwrap();
        if entry == "spawn" {
            assert_eq!(
                runtime
                    .spawn_task(&f.value(scope.capability()), &f.value(factory))
                    .unwrap_err()
                    .kind(),
                RuntimeErrorKind::ExecutionPhaseViolation
            );
        } else {
            let error = f
                .runtime
                .execute(candidate.module(), entry, &[], &Default::default())
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    EmbeddingError::Runtime {
                        kind: RuntimeFailureKind::ExecutionPhaseViolation,
                        ..
                    }
                ),
                "{error:?}"
            );
        }
        drop(session);
        drop(candidate);
        assert_eq!(runtime.ready_tasks(scope).unwrap(), ready);
        assert_eq!(f.starts.load(Ordering::SeqCst), 1);
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn lifecycle_reload_contract() {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut f = Fixture::configured_source(SOURCE, config);
    let scope = f
        .runtime
        .create_task_scope(&f.module, &Default::default(), Arc::new(|_| Ok(())))
        .unwrap();
    let old_key = f.module.key();
    let factory = f.invoke("factory", &[]);
    let future = f.invoke("cold", &[]);
    let captured = f.invoke("captured", &[]);
    let waiting = spawn(&f, &scope, &factory);
    let queued = spawn(&f, &scope, &captured);
    drop(captured);
    assert_eq!(task_boundary(&f, &scope, waiting), TaskDriveResult::Waiting);
    reject_candidate_work(&f, &scope, &factory);
    let replacement = prepare(
        &f,
        &SOURCE.replace("fn version() -> i32 { 1 }", "fn version() -> i32 { 100 }"),
    );
    let current = f
        .runtime
        .reload_program(&f.module, &replacement, Default::default())
        .unwrap();
    assert_ne!(current.key(), old_key);
    f.module = current;
    f.program = replacement;
    assert_eq!(f.value(&f.invoke("version", &[])), Value::I32(100));
    assert_eq!(
        f.starts.load(Ordering::SeqCst),
        1,
        "reload must not drive waiting or queued work"
    );
    assert!(f.runtime.runtime().modules().loaded(old_key).is_some());
    finish_task(&f, &scope, waiting, 1);
    // A Future captured before publication remains cold until its queued factory runs.
    assert_eq!(task_boundary(&f, &scope, queued), TaskDriveResult::Waiting);
    finish_task(&f, &scope, queued, 1);
    // A retained callable can create a new cold Future from its old generation.
    let retained = spawn(&f, &scope, &factory);
    drop(factory);
    assert_eq!(
        task_boundary(&f, &scope, retained),
        TaskDriveResult::Waiting
    );
    finish_task(&f, &scope, retained, 1);
    let execution = f.start_future(&future);
    drop(future);
    assert!(matches!(
        owned_boundary(&f, &execution),
        DriveResult::Waiting
    ));
    reply(&f, 20);
    assert!(matches!(
        owned_boundary(&f, &execution),
        DriveResult::Waiting
    ));
    assert_eq!(*f.inputs.lock().unwrap().last().unwrap(), 21);
    reply(&f, 30);
    let DriveResult::Complete(result) = owned_boundary(&f, &execution) else {
        panic!("completed retained Future")
    };
    assert_eq!(f.value(&result.unwrap()), Value::I32(57));
    drop(execution);
    let fresh_factory = f.invoke("factory", &[]);
    let fresh = spawn(&f, &scope, &fresh_factory);
    drop(fresh_factory);
    assert_eq!(task_boundary(&f, &scope, fresh), TaskDriveResult::Waiting);
    finish_task(&f, &scope, fresh, 100);
    assert_eq!(f.cancels.load(Ordering::SeqCst), 0);
    scope.close();
    f.runtime.runtime().drain_cancelled_tasks().unwrap();
    drop(scope);
    f.runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        f.runtime
            .runtime()
            .modules()
            .retention_counts(old_key)
            .runtime_values,
        0
    );
    assert_eq!(f.runtime.runtime().gc().active_roots(), 0);
}
