//! Notifications carry identities only. Publication never enters script execution.
use crate::{
    gc::roots::RootedValue,
    task::{CancellationCause, ScopeId, TaskId},
};
use kagari_common::cancellation::{CancellationSubscription, CancellationToken};
use std::{
    fmt, mem,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    task::{Wake, Waker},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadyNotice {
    pub scope: ScopeId,
    pub task: Option<TaskId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchUnavailable;

pub trait TaskDispatcher: Send + Sync {
    fn notify(&self, notice: ReadyNotice) -> Result<(), DispatchUnavailable>;
}

impl<F: Fn(ReadyNotice) -> Result<(), DispatchUnavailable> + Send + Sync> TaskDispatcher for F {
    fn notify(&self, notice: ReadyNotice) -> Result<(), DispatchUnavailable> {
        self(notice)
    }
}

pub(crate) struct ScopeControl {
    pub id: ScopeId,
    pub closing: AtomicBool,
    pub owner_dropped: AtomicBool,
    pub failed: AtomicBool,
    pub closed: AtomicBool,
    dispatcher: Mutex<Arc<DispatcherGeneration>>,
    members: Mutex<Vec<Weak<TaskSignal>>>,
    external: OnceLock<CancellationSubscription>,
}

struct DispatcherGeneration {
    callback: Arc<dyn TaskDispatcher>,
}

impl fmt::Debug for ScopeControl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopeControl")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl ScopeControl {
    pub fn new(
        id: ScopeId,
        dispatcher: Arc<dyn TaskDispatcher>,
        cancellation: &CancellationToken,
    ) -> Arc<Self> {
        let control = Arc::new(Self {
            id,
            closing: AtomicBool::new(false),
            owner_dropped: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            dispatcher: Mutex::new(Arc::new(DispatcherGeneration {
                callback: dispatcher,
            })),
            members: Mutex::new(vec![]),
            external: OnceLock::new(),
        });
        let subscription = cancellation.subscribe(Waker::from(Arc::new(ScopeCancellation(
            Arc::downgrade(&control),
        ))));
        control
            .external
            .set(subscription)
            .expect("new scope cancellation");
        control
    }

    pub fn attach(&self, signal: &Arc<TaskSignal>) {
        let mut members = self.members.lock().unwrap_or_else(|e| e.into_inner());
        members.retain(|entry| entry.strong_count() != 0);
        members.push(Arc::downgrade(signal));
    }

    pub fn cancel_members(&self, cause: CancellationCause) {
        let members = self
            .members
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
            .collect::<Vec<_>>();
        for member in members {
            member.cancel(cause);
        }
    }

    pub fn close(&self, cause: CancellationCause) {
        self.closing.store(true, Ordering::Release);
        self.cancel_members(cause);
        let _ = self.notify(None);
    }

    pub fn notify(&self, task: Option<TaskId>) -> Result<(), DispatchUnavailable> {
        if self.failed.load(Ordering::Acquire) {
            return Err(DispatchUnavailable);
        }
        let dispatcher = {
            let dispatcher = self.dispatcher.lock().unwrap_or_else(|e| e.into_inner());
            if self.failed.load(Ordering::Acquire) {
                return Err(DispatchUnavailable);
            }
            dispatcher.clone()
        };
        if catch_unwind(AssertUnwindSafe(|| {
            dispatcher.callback.notify(ReadyNotice {
                scope: self.id,
                task,
            })
        }))
        .is_ok_and(|value| value.is_ok())
        {
            return Ok(());
        }
        let members = {
            let installed = self.dispatcher.lock().unwrap_or_else(|e| e.into_inner());
            if !Arc::ptr_eq(&installed, &dispatcher) {
                // Replacement replays the durable ready set. A late failure of
                // the previous callback cannot poison its replacement.
                return Ok(());
            }
            self.failed.store(true, Ordering::Release);
            self.members
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>()
        };
        for member in members {
            member.cancel(CancellationCause::DispatchFailure);
        }
        Err(DispatchUnavailable)
    }

    pub fn replace_dispatcher(
        &self,
        dispatcher: Arc<dyn TaskDispatcher>,
    ) -> Result<(), DispatchUnavailable> {
        let previous = {
            let mut installed = self.dispatcher.lock().unwrap_or_else(|e| e.into_inner());
            let previous = mem::replace(
                &mut *installed,
                Arc::new(DispatcherGeneration {
                    callback: dispatcher,
                }),
            );
            self.failed.store(false, Ordering::Release);
            previous
        };
        drop(previous);
        let notices = self
            .members
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter_map(Weak::upgrade)
            .filter(|task| task.ready.load(Ordering::Acquire))
            .map(|task| task.id)
            .collect::<Vec<_>>();
        self.notify(None)?;
        for id in notices {
            self.notify(Some(id))?;
        }
        Ok(())
    }
}

struct ScopeCancellation(Weak<ScopeControl>);

impl Wake for ScopeCancellation {
    fn wake(self: Arc<Self>) {
        if let Some(scope) = self.0.upgrade() {
            scope.close(CancellationCause::ScopeClose);
        }
    }
}

#[derive(Debug)]
pub(crate) struct TaskSignal {
    pub id: TaskId,
    pub scope: Weak<ScopeControl>,
    pub ready: AtomicBool,
    pub finished: AtomicBool,
    pub cancellation: CancellationToken,
    cause: AtomicU8,
}

impl TaskSignal {
    pub fn new(id: TaskId, scope: &Arc<ScopeControl>) -> Arc<Self> {
        Arc::new(Self {
            id,
            scope: Arc::downgrade(scope),
            ready: AtomicBool::new(false),
            finished: AtomicBool::new(false),
            cancellation: CancellationToken::default(),
            cause: AtomicU8::new(0),
        })
    }

    pub fn mark_ready(&self) {
        if !self.finished.load(Ordering::Acquire)
            && !self.ready.swap(true, Ordering::AcqRel)
            && let Some(scope) = self.scope.upgrade()
        {
            let _ = scope.notify(Some(self.id));
        }
    }

    pub fn cancel(&self, cause: CancellationCause) {
        if self.finished.load(Ordering::Acquire) {
            return;
        }
        let _ = self
            .cause
            .compare_exchange(0, cause as u8, Ordering::AcqRel, Ordering::Acquire);
        self.cancellation.cancel();
        self.mark_ready();
    }

    pub fn cause(&self) -> Option<CancellationCause> {
        match self.cause.load(Ordering::Acquire) {
            1 => Some(CancellationCause::Explicit),
            2 => Some(CancellationCause::ScopeClose),
            3 => Some(CancellationCause::OwnerDrop),
            4 => Some(CancellationCause::DispatchFailure),
            5 => Some(CancellationCause::Dependency),
            6 => Some(CancellationCause::RuntimeShutdown),
            _ => None,
        }
    }
}

impl Wake for TaskSignal {
    fn wake(self: Arc<Self>) {
        self.mark_ready();
    }
}

#[derive(Debug)]
pub struct TaskScopeOwner {
    pub(crate) control: Arc<ScopeControl>,
    pub(crate) capability: RootedValue,
}

impl TaskScopeOwner {
    pub fn id(&self) -> ScopeId {
        self.control.id
    }

    pub fn capability(&self) -> &RootedValue {
        &self.capability
    }

    pub fn close(&self) {
        self.control.close(CancellationCause::ScopeClose);
    }

    pub fn is_closed(&self) -> bool {
        self.control.closed.load(Ordering::Acquire)
    }

    pub fn replace_dispatcher(
        &self,
        dispatcher: Arc<dyn TaskDispatcher>,
    ) -> Result<(), DispatchUnavailable> {
        self.control.replace_dispatcher(dispatcher)
    }
}

impl Drop for TaskScopeOwner {
    fn drop(&mut self) {
        self.control.owner_dropped.store(true, Ordering::Release);
        self.control.close(CancellationCause::OwnerDrop);
    }
}
