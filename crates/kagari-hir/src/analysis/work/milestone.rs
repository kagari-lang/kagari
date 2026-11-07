//! Controlled verification pause at the first actual body checker entry.
//!
//! This optional instrumentation has no callbacks or process-wide state. A test
//! installs a gate inside one selected worker operation, observes its entry and
//! releases it after testing cancellation. The intentional pause is excluded from
//! cancellation cleanup latency; this is not an unpaused workload measurement.

use std::{
    cell::RefCell,
    error::Error,
    fmt,
    marker::PhantomData,
    rc::Rc,
    sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    time::{Duration, Instant},
};

/// A one-shot pause transferred to the worker that will run the selected query.
pub struct BodyCheckGate {
    started: SyncSender<Instant>,
    resume: Receiver<()>,
}

impl BodyCheckGate {
    /// Returns the worker gate, entry notification and its release sender.
    ///
    /// Dropping the release sender also unblocks cleanup. A retained sender must
    /// release within ten seconds; expiration fails visibly instead of allowing a
    /// stalled test to claim that cancellation completed.
    pub fn new() -> (Self, Receiver<Instant>, SyncSender<()>) {
        let (started, entries) = sync_channel(1);
        let (release, resume) = sync_channel(1);
        (Self { started, resume }, entries, release)
    }
}

struct Active {
    gate: Option<BodyCheckGate>,
}

thread_local! {
    static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) };
}

/// Nested gates would ambiguously attribute a checker entry to two operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlreadyGated;

impl fmt::Display for AlreadyGated {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a HIR body check gate is already active on this thread")
    }
}

impl Error for AlreadyGated {}

/// Thread-confined scope which removes the gate on return or unwind.
pub struct BodyCheckScope {
    _thread: PhantomData<Rc<()>>,
}

impl Drop for BodyCheckScope {
    fn drop(&mut self) {
        ACTIVE.with(|active| *active.borrow_mut() = None);
    }
}

/// Installs one gate for this synchronous operation; nesting preserves the outer gate.
pub fn install(gate: BodyCheckGate) -> Result<BodyCheckScope, AlreadyGated> {
    ACTIVE.with(|active| {
        let mut active = active.borrow_mut();
        if active.is_some() {
            return Err(AlreadyGated);
        }
        *active = Some(Active { gate: Some(gate) });
        Ok(BodyCheckScope {
            _thread: PhantomData,
        })
    })
}

pub(super) fn body_check() {
    let gate = ACTIVE.with(|active| {
        active
            .borrow_mut()
            .as_mut()
            .and_then(|active| active.gate.take())
    });
    let Some(gate) = gate else {
        return;
    };
    if gate.started.try_send(Instant::now()).is_err() {
        return;
    }
    match gate.resume.recv_timeout(Duration::from_secs(10)) {
        Ok(()) | Err(RecvTimeoutError::Disconnected) => {}
        Err(RecvTimeoutError::Timeout) => panic!("body check verification gate was not released"),
    }
}

#[cfg(test)]
mod tests;
