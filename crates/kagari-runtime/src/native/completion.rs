//! Bounded owned completion transport. Publication never borrows or enters a VM.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
};
use std::{
    any::Any,
    fmt,
    marker::PhantomData,
    mem,
    num::NonZeroUsize,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::Waker,
};

type Payload = NativeResult<Box<dyn Any + Send>>;
type Cancel = Box<dyn FnOnce() + Send>;

/// An opaque operation incarnation. Runtime execution ownership is checked by
/// the consumer retaining the reservation, never inferred from an external ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationId {
    registry: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct Slot {
    generation: Option<u64>,
    active: bool,
}

#[derive(Debug)]
struct Registry {
    id: u64,
    capacity: usize,
    slots: Mutex<Vec<Slot>>,
    faulted: AtomicBool,
}

impl Registry {
    fn check(&self) -> NativeResult<()> {
        if self.faulted.load(Ordering::Acquire) {
            Err(RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                "native completion registry is faulted",
            ))
        } else {
            Ok(())
        }
    }

    fn contain<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        match catch_unwind(AssertUnwindSafe(action)) {
            Ok(value) => Some(value),
            Err(panic) => {
                self.faulted.store(true, Ordering::Release);
                // A panic payload is arbitrary host data too. Try to destroy it,
                // but never recursively unwind through a second panic payload.
                if let Err(nested) = catch_unwind(AssertUnwindSafe(|| drop(panic))) {
                    mem::forget(nested);
                }
                None
            }
        }
    }

    fn release(&self, id: OperationId) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        let Some(slot) = slots.get_mut(id.slot) else {
            self.faulted.store(true, Ordering::Release);
            return;
        };
        if id.registry != self.id || slot.generation != Some(id.generation) || !slot.active {
            self.faulted.store(true, Ordering::Release);
            return;
        }
        slot.active = false;
        slot.generation = id.generation.checked_add(1);
    }
}

/// One result slot is reserved before external work is submitted. Slots contain
/// owned host data only; typed conversion into script values belongs to the driver.
#[derive(Debug, Clone)]
pub struct CompletionRegistry {
    inner: Arc<Registry>,
}

impl CompletionRegistry {
    pub fn new(capacity: NonZeroUsize) -> NativeResult<Self> {
        static NEXT_REGISTRY: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_REGISTRY
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| RuntimeError::resource_limit("completion registry identities"))?;
        Ok(Self {
            inner: Arc::new(Registry {
                id,
                capacity: capacity.get(),
                slots: Mutex::new(Vec::new()),
                faulted: AtomicBool::new(false),
            }),
        })
    }

    pub fn reserve<T: Send + 'static>(&self) -> NativeResult<Operation<T>> {
        self.inner.check()?;
        let mut slots = self.inner.slots.lock().unwrap_or_else(|e| e.into_inner());
        let index = match slots
            .iter()
            .position(|slot| !slot.active && slot.generation.is_some())
        {
            Some(index) => index,
            None => {
                if slots.len() == self.inner.capacity {
                    return Err(RuntimeError::resource_limit("native operation capacity"));
                }
                slots
                    .try_reserve(1)
                    .map_err(|_| RuntimeError::resource_limit("native operation slots"))?;
                let index = slots.len();
                slots.push(Slot {
                    generation: Some(0),
                    active: false,
                });
                index
            }
        };
        let slot = &mut slots[index];
        slot.active = true;
        let id = OperationId {
            registry: self.inner.id,
            slot: index,
            generation: slot.generation.expect("available generation"),
        };
        drop(slots);
        Ok(Operation {
            registry: self.inner.clone(),
            lease: Some(Reservation {
                id,
                registry: self.inner.clone(),
            }),
            state: Arc::new(Mutex::new(CompletionState {
                status: Status::Pending,
                waker: None,
            })),
            cancel: None,
            marker: PhantomData,
        })
    }

    pub fn active_count(&self) -> usize {
        self.inner
            .slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|slot| slot.active)
            .count()
    }

    pub fn check(&self) -> NativeResult<()> {
        self.inner.check()
    }
}

struct Reservation {
    id: OperationId,
    registry: Arc<Registry>,
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.registry.release(self.id);
    }
}

enum Status {
    Pending,
    Ready(Payload),
    Retired,
}

struct CompletionState {
    status: Status,
    waker: Option<Arc<Waker>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionStatus {
    Accepted,
    Duplicate,
    Stale,
}

/// A typed, cloneable producer endpoint. The endpoint does not retain a runtime,
/// execution, result payload or active reservation after the consumer retires.
pub struct Completion<T> {
    id: OperationId,
    state: Weak<Mutex<CompletionState>>,
    registry: Weak<Registry>,
    marker: PhantomData<fn(T)>,
}

impl<T> Clone for Completion<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            state: self.state.clone(),
            registry: self.registry.clone(),
            marker: PhantomData,
        }
    }
}

impl<T> fmt::Debug for Completion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Completion")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<T: Send + 'static> Completion<T> {
    pub fn id(&self) -> OperationId {
        self.id
    }

    /// Store before waking. Completion during provider start is indistinguishable
    /// from later completion, and a duplicate never overwrites the first result.
    /// Waker failures fault the registry without undoing publication. Rejected
    /// results remain producer-side data and are destroyed on the calling thread.
    pub fn complete(&self, result: NativeResult<T>) -> CompletionStatus {
        let Some(registry) = self.registry.upgrade() else {
            return CompletionStatus::Stale;
        };
        let Some(state) = self.state.upgrade() else {
            return CompletionStatus::Stale;
        };
        let wake = {
            let mut state = state.lock().unwrap_or_else(|e| e.into_inner());
            match state.status {
                Status::Pending => {}
                Status::Ready(_) => return CompletionStatus::Duplicate,
                Status::Retired => return CompletionStatus::Stale,
            }
            state.status =
                Status::Ready(result.map(|value| Box::new(value) as Box<dyn Any + Send>));
            state.waker.clone()
        };
        if let Some(wake) = wake {
            // Retirement can race publication, leaving this producer with the
            // final waker reference. Contain its callback and destruction apart.
            registry.contain(|| wake.wake_by_ref());
            registry.contain(|| drop(wake));
        }
        CompletionStatus::Accepted
    }
}

#[derive(Debug)]
pub enum OperationPoll<T> {
    Pending,
    Ready(NativeResult<T>),
    Retired,
}

/// The driver owns the single consumer and cancellation hook. Polling never
/// executes script or converts a payload; it only transfers the owned result.
pub struct Operation<T> {
    registry: Arc<Registry>,
    lease: Option<Reservation>,
    state: Arc<Mutex<CompletionState>>,
    cancel: Option<Cancel>,
    marker: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for Operation<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Operation")
            .field("id", &self.lease.as_ref().map(|lease| lease.id))
            .finish_non_exhaustive()
    }
}

impl<T: Send + 'static> Operation<T> {
    pub fn completion(&self) -> NativeResult<Completion<T>> {
        let lease = self
            .lease
            .as_ref()
            .ok_or_else(|| RuntimeError::module_validation("retired operation"))?;
        Ok(Completion {
            id: lease.id,
            state: Arc::downgrade(&self.state),
            registry: Arc::downgrade(&self.registry),
            marker: PhantomData,
        })
    }

    /// Register before parking. Recheck under the publication lock so completion
    /// between the last poll and waker registration cannot be lost. Waker callback
    /// or destruction panics fault the registry; check it before continuing work.
    pub fn set_waker(&self, waker: &Waker) {
        if self.lease.is_none() {
            return;
        }
        let Some(registered) = self.registry.contain(|| Arc::new(waker.clone())) else {
            return;
        };
        let (ready, previous) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let previous = state.waker.replace(registered);
            (matches!(state.status, Status::Ready(_)), previous)
        };
        self.registry.contain(|| drop(previous));
        if ready {
            self.registry.contain(|| waker.wake_by_ref());
        }
    }

    /// Install the provider's bounded, nonblocking cancellation after submission.
    /// A live operation retains its first hook even when returning EngineFault:
    /// submitted work still needs cancellation when the operation is retired.
    pub fn on_cancel(&mut self, cancel: impl FnOnce() + Send + 'static) -> NativeResult<()> {
        if self.lease.is_none() || self.cancel.is_some() {
            self.registry.contain(|| drop(cancel));
            self.registry.check()?;
            return Err(RuntimeError::module_validation(
                "operation cancellation hook state",
            ));
        }
        self.cancel = Some(Box::new(cancel));
        self.registry.check()
    }

    pub fn poll(&mut self) -> NativeResult<OperationPoll<T>> {
        self.registry.check()?;
        let (payload, wake) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match state.status {
                Status::Pending => return Ok(OperationPoll::Pending),
                Status::Retired => return Ok(OperationPoll::Retired),
                Status::Ready(_) => {}
            }
            let Status::Ready(payload) = mem::replace(&mut state.status, Status::Retired) else {
                unreachable!()
            };
            (payload, state.waker.take())
        };
        // Detach all terminal ownership before any host destruction. A claim
        // disarms the hook even if destroying its captures or the waker fails.
        let lease = self.lease.take();
        let cancel = self.cancel.take();
        self.registry.contain(|| drop(wake));
        self.registry.contain(|| drop(cancel));
        if let Err(error) = self.registry.check() {
            self.registry.contain(|| drop(payload));
            drop(lease);
            return Err(error);
        }
        let result = self.registry.contain(|| {
            payload.and_then(|value| {
                value.downcast::<T>().map(|value| *value).map_err(|value| {
                    self.registry.faulted.store(true, Ordering::Release);
                    drop(value);
                    RuntimeError::new(
                        RuntimeErrorKind::EngineFault,
                        "native completion payload type",
                    )
                })
            })
        });
        drop(lease);
        if let Err(error) = self.registry.check() {
            self.registry.contain(|| drop(result));
            return Err(error);
        }
        Ok(OperationPoll::Ready(
            result.expect("contained payload claim"),
        ))
    }
}

impl<T> Operation<T> {
    /// Retire once and drain all transport-owned cleanup outside its locks.
    /// Cleanup continues even if the registry was already faulted. Any fault is
    /// reported as EngineFault after releasing the reservation, including on retries.
    pub fn cancel(&mut self) -> NativeResult<()> {
        let previous = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let wake = state.waker.take();
            (mem::replace(&mut state.status, Status::Retired), wake)
        };
        let lease = self.lease.take();
        let cancel = self.cancel.take();
        let (status, wake) = previous;
        self.registry.contain(|| drop(status));
        self.registry.contain(|| drop(wake));
        if let Some(cancel) = cancel {
            self.registry.contain(cancel);
        }
        drop(lease);
        self.registry.check()
    }
}

impl<T> Drop for Operation<T> {
    fn drop(&mut self) {
        let _ = self.cancel();
    }
}

#[cfg(test)]
mod tests;
