//! This same host is exercised by the independent source-free feature consumer.
use super::provider::FakeIo;
use kagari_embed::{
    BytecodeArtifact,
    engine::KagariEngine,
    error::EmbeddingError,
    program::PreparedProgram,
    runtime::{KagariRuntime, owned::DriveResult},
};
use kagari_runtime::{
    error::RuntimeErrorKind,
    module::LoadedModule,
    native::completion::CompletionStatus,
    task::{CancellationCause, TaskId, control::TaskScopeOwner, drive::TaskDriveResult},
    value::Value,
};
use std::{
    error::Error,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

type HostResult<T> = Result<T, Box<dyn Error>>;

// EmbeddingError is a structured SDK diagnostic, not a std::error::Error.
fn diagnostic(error: EmbeddingError) -> Box<dyn Error> {
    format!("{error:?}").into()
}

fn submit(
    runtime: &KagariRuntime,
    module: &LoadedModule,
    scope: &TaskScopeOwner,
) -> HostResult<TaskId> {
    let capability = scope
        .capability()
        .value(runtime.runtime().gc())
        .ok_or("scope root")?;
    let handler = runtime
        .start(module, "handle", &[capability], &Default::default())
        .map_err(diagnostic)?;
    for _ in 0..1000 {
        match runtime
            .drive(&handler, NonZeroUsize::new(16).unwrap())
            .map_err(diagnostic)?
        {
            DriveResult::Runnable => {}
            DriveResult::Waiting => return Err("synchronous handler unexpectedly waited".into()),
            DriveResult::Complete(result) => {
                if result.map_err(diagnostic)?.value(runtime.runtime().gc())
                    != Some(Value::Bool(true))
                {
                    return Err("scope rejected admission".into());
                }
                let tasks = runtime.runtime().ready_tasks(scope)?;
                return match tasks.as_slice() {
                    [task] => Ok(*task),
                    _ => Err("expected one admitted task".into()),
                };
            }
        }
    }
    Err("handler exceeded demo drive bound".into())
}

fn drive(
    runtime: &KagariRuntime,
    scope: &TaskScopeOwner,
    task: TaskId,
) -> HostResult<TaskDriveResult> {
    for _ in 0..1000 {
        let outcome = runtime
            .drive_task(scope, task, NonZeroUsize::new(16).unwrap())
            .map_err(diagnostic)?;
        runtime.runtime().collect_garbage()?;
        if outcome != TaskDriveResult::Runnable {
            return Ok(outcome);
        }
    }
    Err("task exceeded demo drive bound".into())
}

pub fn run(engine: KagariEngine, artifact: BytecodeArtifact, io: FakeIo) -> HostResult<()> {
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())?;
    let mut runtime = engine.runtime(Default::default());
    let module = runtime
        .load_program(&program, Default::default())
        .map_err(diagnostic)?;
    // A host event loop can turn this notification into a mailbox event or wake.
    // This small non-Actor dispatcher records notices and never drives script.
    let notices = Arc::new(AtomicUsize::new(0));
    let notified = notices.clone();
    let scope = runtime
        .create_task_scope(
            &module,
            &Default::default(),
            Arc::new(move |_| {
                notified.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
        )
        .map_err(diagnostic)?;
    let task = submit(&runtime, &module, &scope)?;
    assert_eq!(
        io.started(),
        0,
        "handler returns before starting its factory"
    );
    assert!(notices.load(Ordering::SeqCst) > 0);
    assert_eq!(drive(&runtime, &scope, task)?, TaskDriveResult::Waiting);
    for (index, (service, input, reply)) in [
        ("demo::rpc", 1, 10),
        ("demo::database", 10, 100),
        ("demo::rpc", 2, 20),
        ("demo::database", 20, 200),
    ]
    .into_iter()
    .enumerate()
    {
        let request = io.take_request().ok_or("missing fake request")?;
        assert_eq!((request.service, request.input), (service, input));
        // Independent synchronous work proceeds while the task waits for IO.
        let ping = runtime
            .execute(&module, "ping", &[], &Default::default())
            .map_err(diagnostic)?;
        assert_eq!(
            ping.return_value.value(runtime.runtime().gc()),
            Some(Value::I32(42))
        );
        drop(ping);
        let before = notices.load(Ordering::SeqCst);
        assert_eq!(
            request.completion.complete(Ok(reply)),
            CompletionStatus::Accepted
        );
        assert!(notices.load(Ordering::SeqCst) > before);
        assert_eq!(
            io.started(),
            index + 1,
            "readiness alone cannot resume script"
        );
        assert_eq!(
            drive(&runtime, &scope, task)?,
            if index == 3 {
                TaskDriveResult::Complete
            } else {
                TaskDriveResult::Waiting
            }
        );
    }
    let report = runtime
        .runtime()
        .take_task_report(&scope, task)?
        .ok_or("missing report")?;
    assert_eq!(report.task, task);
    assert_eq!(report.scope, scope.id());
    let value = report.outcome.map_err(|failure| failure.error)?;
    assert_eq!(value.value(runtime.runtime().gc()), Some(Value::I32(300)));
    drop(value);
    println!("Task {task:?}: completed with 300 after two RPC/database pairs");

    let cancelled = submit(&runtime, &module, &scope)?;
    assert_eq!(
        drive(&runtime, &scope, cancelled)?,
        TaskDriveResult::Waiting
    );
    let late = io.take_request().ok_or("missing cancellable request")?;
    scope.close();
    assert!(
        !scope.is_closed(),
        "requesting cancellation is not cleanup acknowledgement"
    );
    assert_eq!(runtime.runtime().drain_cancelled_tasks()?, 1);
    assert!(scope.is_closed());
    let failure = runtime
        .runtime()
        .take_task_report(&scope, cancelled)?
        .ok_or("missing cancellation report")?
        .outcome
        .expect_err("scope close cancels pending work");
    assert_eq!(failure.source_task, cancelled);
    assert_eq!(failure.error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(failure.cancellation, Some(CancellationCause::ScopeClose));
    assert_eq!(io.cancelled(), 1);
    assert_eq!(late.completion.complete(Ok(999)), CompletionStatus::Stale);
    println!("Task {cancelled:?}: cancelled by scope close; late reply rejected");
    drop(scope);
    runtime.runtime().drain_cancelled_tasks()?;
    runtime.runtime().collect_garbage()?;
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    Ok(())
}
