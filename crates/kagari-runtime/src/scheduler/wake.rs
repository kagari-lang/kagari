//! Notification-only routing; neither host wakers nor worker threads enter the heap.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
};
use std::{
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Wake, Waker},
};

#[derive(Debug, Default)]
struct Routes {
    external: Option<Arc<Waker>>,
    borrowed: Option<(u64, Arc<Waker>)>,
    serial: u64,
    closed: bool,
}

#[derive(Debug, Default)]
pub(crate) struct WakeHub {
    routes: Mutex<Routes>,
    dirty: AtomicBool,
    faulted: AtomicBool,
}

/// Temporary notification route for an exclusively borrowed driving call.
/// Dropping it restores the persistent external route and replays durable readiness.
/// It owns notification state only and cannot drive or retain a runtime.
#[derive(Debug)]
pub struct DriverRegistration {
    hub: Weak<WakeHub>,
    serial: u64,
}

impl Drop for DriverRegistration {
    fn drop(&mut self) {
        let Some(hub) = self.hub.upgrade() else {
            return;
        };
        let previous = {
            let mut routes = hub.routes.lock().unwrap_or_else(|e| e.into_inner());
            if routes
                .borrowed
                .as_ref()
                .is_none_or(|(serial, _)| *serial != self.serial)
            {
                return;
            }
            routes.borrowed.take()
        };
        hub.contain(|| drop(previous));
        if hub.dirty.load(Ordering::Acquire) {
            hub.notify();
        }
    }
}

impl WakeHub {
    pub(crate) fn contain<T>(&self, action: impl FnOnce() -> T) -> Option<T> {
        self.contain_callback(action, true)
    }

    fn contain_callback<T>(&self, action: impl FnOnce() -> T, notify_fault: bool) -> Option<T> {
        match catch_unwind(AssertUnwindSafe(action)) {
            Ok(value) => Some(value),
            Err(payload) => {
                let first_fault = !self.faulted.swap(true, Ordering::AcqRel);
                self.dirty.store(true, Ordering::Release);
                if let Err(nested) = catch_unwind(AssertUnwindSafe(|| drop(payload))) {
                    mem::forget(nested);
                }
                // A receiver may fail on another thread while every execution is
                // waiting. Fault readiness must wake independently of dirty's
                // coalescing bit. Notification callbacks cannot retry themselves.
                if first_fault && notify_fault {
                    self.notify();
                }
                None
            }
        }
    }

    pub(crate) fn check(&self) -> NativeResult<()> {
        if self.faulted.load(Ordering::Acquire) {
            Err(RuntimeError::new(
                RuntimeErrorKind::EngineFault,
                "scheduler callback failed",
            ))
        } else {
            Ok(())
        }
    }

    fn notify(&self) {
        let wake = {
            let routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            if routes.closed {
                return;
            }
            routes
                .borrowed
                .as_ref()
                .map(|(_, wake)| wake)
                .or(routes.external.as_ref())
                .cloned()
        };
        if let Some(wake) = wake {
            self.contain_callback(|| wake.wake_by_ref(), false);
            self.contain_callback(|| drop(wake), false);
        }
    }

    pub(crate) fn mark_ready(&self) {
        if !self.dirty.swap(true, Ordering::AcqRel) {
            self.notify();
        }
    }

    pub(crate) fn begin_turn(&self) {
        self.dirty.store(false, Ordering::Release);
    }

    pub(crate) fn finish_turn(&self, runnable: bool) {
        if runnable {
            self.dirty.store(true, Ordering::Release);
        }
        if self.dirty.load(Ordering::Acquire) {
            self.notify();
        }
    }

    pub(crate) fn register_external(&self, wake: &Waker) -> NativeResult<()> {
        let mut registered = self.contain(|| Arc::new(wake.clone()));
        let previous = {
            let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            if routes.closed {
                None
            } else {
                mem::replace(&mut routes.external, registered.take())
            }
        };
        self.contain(|| drop(registered));
        self.contain(|| drop(previous));
        self.check()
    }

    pub(crate) fn borrow(self: &Arc<Self>, wake: &Waker) -> NativeResult<DriverRegistration> {
        let mut registered = self.contain(|| Arc::new(wake.clone()));
        if let Err(error) = self.check() {
            self.contain(|| drop(registered));
            return Err(error);
        }
        let serial = (|| {
            let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            if routes.closed || routes.borrowed.is_some() {
                return Err(RuntimeError::module_validation(
                    "runtime driver route unavailable",
                ));
            }
            routes.serial = routes
                .serial
                .checked_add(1)
                .ok_or_else(|| RuntimeError::resource_limit("driver registrations"))?;
            let serial = routes.serial;
            routes.borrowed = Some((serial, registered.take().expect("checked waker")));
            Ok(serial)
        })();
        self.contain(|| drop(registered));
        let serial = serial?;
        if self.dirty.load(Ordering::Acquire) {
            self.notify();
        }
        Ok(DriverRegistration {
            hub: Arc::downgrade(self),
            serial,
        })
    }

    pub(crate) fn update(
        &self,
        registration: &DriverRegistration,
        wake: &Waker,
    ) -> NativeResult<()> {
        if !ptr::eq(registration.hub.as_ptr(), self) {
            return Err(RuntimeError::module_validation(
                "foreign driver registration",
            ));
        }
        let mut registered = self.contain(|| Arc::new(wake.clone()));
        if let Err(error) = self.check() {
            self.contain(|| drop(registered));
            return Err(error);
        }
        let previous = (|| {
            let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            let Some((serial, current)) = &mut routes.borrowed else {
                return Err(RuntimeError::module_validation(
                    "retired driver registration",
                ));
            };
            if *serial != registration.serial {
                return Err(RuntimeError::module_validation("stale driver registration"));
            }
            Ok(mem::replace(
                current,
                registered.take().expect("checked waker"),
            ))
        })();
        self.contain(|| drop(registered));
        let previous = previous?;
        self.contain(|| drop(previous));
        self.check()
    }

    pub(crate) fn close(&self) {
        let previous = {
            let mut routes = self.routes.lock().unwrap_or_else(|e| e.into_inner());
            routes.closed = true;
            (routes.external.take(), routes.borrowed.take())
        };
        self.contain(|| drop(previous.0));
        self.contain(|| drop(previous.1));
    }
}

impl Wake for WakeHub {
    fn wake(self: Arc<Self>) {
        self.mark_ready();
    }
}
