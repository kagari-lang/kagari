//! Driving calls borrow Runtime; submitted calls retain only result/cancellation state.
use crate::{RunResult, error::EmbeddingError, runtime::Runtime};
use kagari_runtime::scheduler::{TurnBudget, result::ScheduledResult, wake::DriverRegistration};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// Result-only submitted invocation. Polling never executes script. Keep driving
/// Runtime separately; drop requests cancellation and runtime teardown resolves it.
#[must_use = "drive Runtime and await this result, or drop it to request cancellation"]
#[derive(Debug)]
pub struct PendingCall<R> {
    pub(super) result: ScheduledResult<R>,
}

impl<R> PendingCall<R> {
    pub fn is_finished(&self) -> bool {
        self.result.is_finished()
    }

    pub fn cancel(&self) {
        self.result.cancel();
    }
}

impl<R> Future for PendingCall<R> {
    type Output = RunResult<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().result)
            .poll(cx)
            .map(|result| result.map_err(EmbeddingError::from))
    }
}

/// A direct-await invocation that drives the common bounded scheduler. Its
/// exclusive borrow prevents other direct access until completion or drop.
/// Dropping unfinished work synchronously releases local frames and descendants,
/// even if no external driver was ever installed. Cleanup faults quarantine Runtime.
#[must_use = "await the call to drive it; dropping it cancels and cleans up locally"]
pub struct Call<'runtime, R> {
    runtime: &'runtime mut Runtime,
    pending: PendingCall<R>,
    driver: Option<DriverRegistration>,
    finished: bool,
}

impl<'runtime, R> Call<'runtime, R> {
    pub(super) fn new(runtime: &'runtime mut Runtime, pending: PendingCall<R>) -> Self {
        Self {
            runtime,
            pending,
            driver: None,
            finished: false,
        }
    }

    fn finish(&mut self, result: RunResult<R>) -> Poll<RunResult<R>> {
        self.finished = true;
        self.driver.take();
        if let Err(fault) = self.runtime.runtime().check_scheduler_health() {
            let mut failure = EmbeddingError::from(fault);
            match result {
                Ok(output) => self.pending.result.discard_output(output),
                Err(EmbeddingError::Runtime {
                    trace, task_origin, ..
                }) => {
                    if let EmbeddingError::Runtime {
                        trace: failure_trace,
                        task_origin: failure_origin,
                        ..
                    } = &mut failure
                    {
                        *failure_trace = trace;
                        *failure_origin = task_origin;
                    }
                }
                Err(_) => {}
            }
            let _ = self.runtime.close();
            return Poll::Ready(Err(failure));
        }
        Poll::Ready(result)
    }
}

impl<R> Future for Call<'_, R> {
    type Output = RunResult<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.finished, "completed Call polled again");
        if this.driver.is_none() {
            match this.runtime.runtime().borrow_scheduler_driver(cx) {
                Ok(driver) => this.driver = Some(driver),
                Err(error) => {
                    let _ = this
                        .runtime
                        .runtime()
                        .cancel_scheduled_execution(&this.pending.result);
                    return this.finish(Err(error.into()));
                }
            }
        }
        if let Poll::Ready(result) = Pin::new(&mut this.pending).poll(cx) {
            return this.finish(result);
        }
        let turn = this
            .runtime
            .poll_scheduler(cx, TurnBudget::default(), this.driver.as_ref());
        if let Poll::Ready(result) = Pin::new(&mut this.pending).poll(cx) {
            return this.finish(result);
        }
        if let Poll::Ready(Err(error)) = turn {
            let _ = this
                .runtime
                .runtime()
                .cancel_scheduled_execution(&this.pending.result);
            return this.finish(Err(error));
        }
        Poll::Pending
    }
}

impl<R> Drop for Call<'_, R> {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self
                .runtime
                .runtime()
                .cancel_scheduled_execution(&self.pending.result);
        }
        self.driver.take();
        if self.runtime.runtime().check_scheduler_health().is_err() {
            let _ = self.runtime.close();
        }
    }
}

#[cfg(all(test, feature = "source"))]
mod tests {
    use super::Call;
    use crate::{
        engine::KagariEngine,
        error::{EmbeddingError, RuntimeFailureKind},
    };
    use kagari_runtime::{
        frame::types::arguments::TypeArgument,
        native::{
            binding::NativeResult,
            catalog::DeclarationCatalog,
            conversion::{FromKagari, KagariType, context::ConversionContext},
            types::Type,
        },
        value::Value,
    };
    use kagari_source::source::SourceFile;
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        task::{Context, Poll, Wake, Waker},
    };

    static DISCARDED: AtomicUsize = AtomicUsize::new(0);

    struct HostileOutput;

    impl KagariType for HostileOutput {
        fn kagari_type(catalog: &DeclarationCatalog) -> NativeResult<Type> {
            i32::kagari_type(catalog)
        }
    }

    impl FromKagari for HostileOutput {
        fn from_kagari(
            cx: &mut ConversionContext<'_>,
            ty: &TypeArgument,
            value: &Value,
        ) -> NativeResult<Self> {
            assert_eq!(i32::from_kagari(cx, ty, value)?, 42);
            Ok(Self)
        }
    }

    impl Drop for HostileOutput {
        fn drop(&mut self) {
            DISCARDED.fetch_add(1, Ordering::SeqCst);
            panic!("discarded host output");
        }
    }

    struct HostileDriverDrop(Arc<AtomicUsize>);

    // The retained waker's destructor is under test; noop has no owned destructor.
    #[expect(clippy::manual_noop_waker, reason = "exercise host waker destruction")]
    impl Wake for HostileDriverDrop {
        fn wake(self: Arc<Self>) {}
    }

    impl Drop for HostileDriverDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
            panic!("driver release failed");
        }
    }

    #[test]
    fn driver_release_fault_overrides_completion_and_preserves_trace() {
        let engine = KagariEngine::default();
        let program = engine
            .compile_to_program(
                SourceFile::new(
                    "handoff.kgr",
                    r#"
            pub async fn run(fail:bool) -> i32 {
                if fail { val zero=0; 1/zero } else { 42 }
            }
        "#,
                ),
                Default::default(),
            )
            .unwrap();
        for fail in [false, true] {
            let mut runtime = engine.runtime(Default::default());
            let module = runtime.load_program(&program, Default::default()).unwrap();
            let function = runtime
                .async_function::<(bool,), HostileOutput>(&module, "run")
                .unwrap();
            let discarded = DISCARDED.load(Ordering::SeqCst);
            let pending = runtime.submit_async(&function, (fail,)).unwrap();
            for _ in 0..100 {
                if pending.is_finished() {
                    break;
                }
                assert!(
                    runtime
                        .poll(&mut Context::from_waker(Waker::noop()))
                        .is_pending()
                );
            }
            assert!(pending.is_finished());
            let mut sibling = runtime.submit_async(&function, (false,)).unwrap();
            let drops = Arc::new(AtomicUsize::new(0));
            let wake = Waker::from(Arc::new(HostileDriverDrop(drops.clone())));
            let driver = runtime
                .runtime()
                .borrow_scheduler_driver(&Context::from_waker(&wake))
                .unwrap();
            drop(wake);
            // A ready result bypasses another scheduler turn. Releasing the only
            // driver-waker owner in finish is the first operation that faults.
            let mut call = Call::new(&mut runtime, pending);
            call.driver = Some(driver);
            let Poll::Ready(result) =
                Pin::new(&mut call).poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("completed invocation");
            };
            drop(call);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let Err(EmbeddingError::Runtime { kind, trace, .. }) = result else {
                panic!("driver cleanup failure must override success");
            };
            assert_eq!(kind, RuntimeFailureKind::EngineInvariant);
            assert_eq!(
                DISCARDED.load(Ordering::SeqCst),
                discarded + usize::from(!fail)
            );
            if fail {
                assert!(
                    trace.is_some(),
                    "preserve the original execution failure trace"
                );
            }
            assert!(runtime.runtime().scheduler_is_closed());
            assert!(matches!(
                Pin::new(&mut sibling).poll(&mut Context::from_waker(Waker::noop())),
                Poll::Ready(Err(_))
            ));
        }
    }
}
