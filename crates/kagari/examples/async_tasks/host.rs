//! This same host is exercised by the independent source-free feature consumer.
use super::provider::FakeIo;
use kagari::{engine::KagariEngine, error::EmbeddingError, program::Program, runtime::Runtime};
use kagari_runtime::{
    error::RuntimeErrorKind,
    native::{completion::CompletionStatus, objects::Object},
    scheduler::tasks::TaskEvent,
    task::{CancellationCause, value::TaskScope},
};
use std::{
    error::Error,
    sync::{
        Arc,
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

type HostResult<T> = Result<T, Box<dyn Error>>;

fn diagnostic(error: impl Into<EmbeddingError>) -> Box<dyn Error> {
    let error = error.into();
    format!("{error:?}").into()
}

struct Notify(SyncSender<()>);

impl Wake for Notify {
    fn wake(self: Arc<Self>) {
        match self.0.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => panic!("host event loop disconnected"),
        }
    }
}

// The capacity-one mailbox coalesces, rather than discarding, readiness. The host
// closes the runtime on receive failure; Waker itself cannot report disconnection.
fn turn(runtime: &mut Runtime, wake: &Waker, ready: &Receiver<()>) -> HostResult<()> {
    if let Err(error) = ready.recv_timeout(Duration::from_secs(2)) {
        runtime.close().map_err(diagnostic)?;
        return Err(error.into());
    }
    match runtime.poll(&mut Context::from_waker(wake)) {
        Poll::Pending => Ok(()),
        Poll::Ready(Err(error)) => Err(diagnostic(error)),
        Poll::Ready(Ok(())) => Err("runtime unexpectedly closed".into()),
    }
}

fn settle(runtime: &mut Runtime, wake: &Waker, ready: &Receiver<()>) -> HostResult<()> {
    // A bounded scheduler turn can re-notify itself before reaching its IO wait.
    // Consume only genuine notifications, never drive Tasks by their IDs.
    for _ in 0..1000 {
        if ready.try_recv().is_err() {
            return Ok(());
        }
        match runtime.poll(&mut Context::from_waker(wake)) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                result.map_err(diagnostic)?;
                return Err("closed runtime".into());
            }
        }
    }
    Err("demo exceeded notification bound".into())
}

pub fn run(engine: KagariEngine, program: Program, io: FakeIo) -> HostResult<()> {
    let mut runtime = engine.runtime(Default::default());
    let module = runtime
        .load_program(&program, Default::default())
        .map_err(diagnostic)?;
    runtime.collect_garbage()?;
    let baseline = runtime.gc_stats();
    let (send, ready) = mpsc::sync_channel(1);
    let wake = Waker::from(Arc::new(Notify(send)));
    assert!(runtime.poll(&mut Context::from_waker(&wake)).is_pending());
    let scope = runtime.create_task_scope(&module).map_err(diagnostic)?;
    let make = runtime
        .bind_function::<(), Object>(&module.root(), "job")
        .map_err(diagnostic)?;
    let handler = runtime
        .bind_function::<(TaskScope, Object), bool>(&module.root(), "handle")
        .map_err(diagnostic)?;
    let ping = runtime
        .bind_function::<(), i32>(&module.root(), "ping")
        .map_err(diagnostic)?;
    let job = runtime.call(&make, ()).map_err(diagnostic)?;
    let total = runtime
        .bind_field::<i32>(job.object_type(), "total")
        .map_err(diagnostic)?;
    assert!(
        runtime
            .call(&handler, (scope.handle(), job.clone()))
            .map_err(diagnostic)?
    );
    assert_eq!(
        io.started(),
        0,
        "handler returns before starting its factory"
    );
    turn(&mut runtime, &wake, &ready)?;
    settle(&mut runtime, &wake, &ready)?;
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
        assert_eq!(runtime.call(&ping, ()).map_err(diagnostic)?, 42);
        assert_eq!(
            request.completion.complete(Ok(reply)),
            CompletionStatus::Accepted
        );
        assert_eq!(
            io.started(),
            index + 1,
            "readiness alone cannot resume script"
        );
        turn(&mut runtime, &wake, &ready)?;
        settle(&mut runtime, &wake, &ready)?;
    }
    assert_eq!(runtime.get(&job, &total).map_err(diagnostic)?, 300);
    assert!(runtime.take_task_event().is_none());
    println!("Task completed with 300 after two RPC/database pairs");

    assert!(
        runtime
            .call(&handler, (scope.handle(), job.clone()))
            .map_err(diagnostic)?
    );
    turn(&mut runtime, &wake, &ready)?;
    settle(&mut runtime, &wake, &ready)?;
    let late = io.take_request().ok_or("missing cancellable request")?;
    scope.close();
    assert!(
        !scope.is_closed(),
        "cancellation request is not cleanup acknowledgement"
    );
    turn(&mut runtime, &wake, &ready)?;
    settle(&mut runtime, &wake, &ready)?;
    assert!(scope.is_closed());
    let Some(TaskEvent::Failed(event)) = runtime.take_task_event() else {
        return Err("missing cancellation event".into());
    };
    assert_eq!(event.scope, scope.id());
    assert_eq!(event.failure.source_task, event.task);
    assert_eq!(event.failure.error.kind(), RuntimeErrorKind::Cancelled);
    assert_eq!(
        event.failure.cancellation,
        Some(CancellationCause::ScopeClose)
    );
    assert_eq!(io.cancelled(), 1);
    assert_eq!(late.completion.complete(Ok(999)), CompletionStatus::Stale);
    println!("Task cancelled by scope close; late reply rejected");
    drop((scope, job, make, handler, ping, total));
    // Current publication owns prepared environments and String constants. Its
    // cold equivalent lets the host check execution cleanup independently.
    let selected = runtime.select_program(&module).map_err(diagnostic)?;
    let replacement = runtime
        .stage_reload_verified_program(&module, None, selected.verified_program().clone())
        .map_err(diagnostic)?;
    runtime
        .publish_staged_reload(replacement)
        .map_err(diagnostic)?;
    runtime.collect_garbage()?;
    assert!(runtime.validate_executable_module(&selected).is_err());
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    runtime.close().map_err(diagnostic)?;
    Ok(())
}
