//! Per-invocation result transport. Polling observes owned output, never script state.
use crate::{
    Runtime,
    error::{RuntimeError, RuntimeErrorKind},
    gc::roots::RootedValue,
    native::binding::NativeResult,
    scheduler::wake::WakeHub,
    session::ExecutionId,
};
use kagari_common::cancellation::CancellationToken;
use std::{
    fmt,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

struct State<R> {
    result: Option<NativeResult<R>>,
    waker: Option<Arc<Waker>>,
    abandoned: bool,
}

pub(crate) struct ResultCell<R> {
    state: Mutex<State<R>>,
    finished: AtomicBool,
    hub: Arc<WakeHub>,
}

/// Low-level result receiver for a scheduler-owned execution.
/// It does not borrow or drive Runtime. Drop requests cancellation; synchronous
/// local cleanup is available through `Runtime::cancel_scheduled_execution`.
/// A completed result survives runtime teardown; retained handles do not grant
/// access to a destroyed heap.
pub struct ScheduledResult<R> {
    pub(crate) id: ExecutionId,
    pub(crate) cell: Arc<ResultCell<R>>,
    cancellation: CancellationToken,
    consumed: bool,
}

impl<R> fmt::Debug for ScheduledResult<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledResult")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<R> ScheduledResult<R> {
    pub fn execution_id(&self) -> ExecutionId {
        self.id
    }

    pub fn is_finished(&self) -> bool {
        self.cell.finished.load(Ordering::Acquire)
    }

    pub fn cancel(&self) {
        if !self.is_finished() {
            self.cell.hub.contain(|| self.cancellation.cancel());
            self.cell.hub.mark_ready();
        }
    }

    pub(crate) fn belongs_to(&self, hub: &Arc<WakeHub>) -> bool {
        Arc::ptr_eq(&self.cell.hub, hub)
    }
}

impl<R> Future for ScheduledResult<R> {
    type Output = NativeResult<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        assert!(!this.consumed, "completed execution result polled again");
        let mut registered = this.cell.hub.contain(|| Arc::new(cx.waker().clone()));
        let (result, previous) = {
            let mut state = this.cell.state.lock().unwrap_or_else(|e| e.into_inner());
            let previous = state.waker.take();
            let result = state.result.take();
            if result.is_none() {
                state.waker = registered.take();
            }
            (result, previous)
        };
        this.cell.hub.contain(|| drop(previous));
        this.cell.hub.contain(|| drop(registered));
        match result {
            Some(result) => {
                this.consumed = true;
                Poll::Ready(result)
            }
            None => match this.cell.hub.check() {
                Ok(()) => Poll::Pending,
                Err(error) => {
                    this.cancel();
                    this.consumed = true;
                    Poll::Ready(Err(error))
                }
            },
        }
    }
}

impl<R> Drop for ScheduledResult<R> {
    fn drop(&mut self) {
        let discarded = {
            let mut state = self.cell.state.lock().unwrap_or_else(|e| e.into_inner());
            state.abandoned = true;
            (state.result.take(), state.waker.take())
        };
        self.cancel();
        self.cell.hub.contain(|| drop(discarded.0));
        self.cell.hub.contain(|| drop(discarded.1));
    }
}

pub(crate) trait CompletionSink: Send {
    fn prepare(&mut self, runtime: &Runtime, value: &RootedValue) -> NativeResult<()>;

    fn publish(self: Box<Self>, outcome: NativeResult<()>);
}

struct TypedSink<R, F> {
    cell: Arc<ResultCell<R>>,
    convert: Option<F>,
    prepared: Option<R>,
}

impl<R: Send, F> CompletionSink for TypedSink<R, F>
where
    F: FnOnce(&Runtime, &RootedValue) -> NativeResult<R> + Send,
{
    fn prepare(&mut self, runtime: &Runtime, value: &RootedValue) -> NativeResult<()> {
        if self
            .cell
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .abandoned
        {
            return Err(RuntimeError::new(
                RuntimeErrorKind::Cancelled,
                "execution receiver dropped",
            ));
        }
        self.prepared = Some(self.convert.take().expect("single result conversion")(
            runtime, value,
        )?);
        Ok(())
    }

    fn publish(mut self: Box<Self>, outcome: NativeResult<()>) {
        self.cell.hub.contain(|| drop(self.convert.take()));
        if outcome.is_err() {
            self.cell.hub.contain(|| drop(self.prepared.take()));
        }
        let outcome = self
            .cell
            .hub
            .check()
            .map_err(
                |fault| match outcome.as_ref().err().and_then(RuntimeError::trace) {
                    Some(trace) => fault.with_trace(trace.clone()),
                    None => fault,
                },
            )
            .and(outcome);
        let result = outcome.map(|()| self.prepared.take().expect("converted execution output"));
        let (wake, discarded) = {
            let mut state = self.cell.state.lock().unwrap_or_else(|e| e.into_inner());
            let discarded = if state.abandoned {
                Some(result)
            } else {
                state.result = Some(result);
                None
            };
            self.cell.finished.store(true, Ordering::Release);
            (state.waker.take(), discarded)
        };
        if let Some(wake) = wake {
            self.cell.hub.contain(|| wake.wake_by_ref());
            self.cell.hub.contain(|| drop(wake));
        }
        self.cell.hub.contain(|| drop(discarded));
        self.cell.hub.contain(|| drop(self.prepared.take()));
        self.cell.hub.contain(|| drop(self.convert.take()));
    }
}

pub(crate) fn channel<R, F>(
    id: ExecutionId,
    hub: Arc<WakeHub>,
    cancellation: CancellationToken,
    convert: F,
) -> (ScheduledResult<R>, Box<dyn CompletionSink>)
where
    R: Send + 'static,
    F: FnOnce(&Runtime, &RootedValue) -> NativeResult<R> + Send + 'static,
{
    let cell = Arc::new(ResultCell {
        state: Mutex::new(State {
            result: None,
            waker: None,
            abandoned: false,
        }),
        finished: AtomicBool::new(false),
        hub,
    });
    let sink = TypedSink {
        cell: cell.clone(),
        convert: Some(convert),
        prepared: None,
    };
    (
        ScheduledResult {
            id,
            cell,
            cancellation,
            consumed: false,
        },
        Box::new(sink),
    )
}
