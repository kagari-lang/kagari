//! Optional operation-scoped measurements of work actually performed on this thread.
//!
//! This instrumentation is excluded from ordinary builds. Counts describe checker
//! entry/restore events, including work before cancellation, rather than provenance
//! stored in cached results. Timings include nested phases and must not be summed
//! as exclusive costs. Measurements never retain source, identity or result data.

use std::{cell::Cell, error::Error, fmt, marker::PhantomData, rc::Rc, time::Instant};

pub mod milestone;

/// Actual operations observed while one synchronous closure executes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Work {
    pub full_snapshot_calls: u64,
    pub selected_body_calls: u64,
    pub shared_body_hits: u64,
    pub body_checks_started: u64,
    pub bodies_restored: u64,
    pub signature_checks_started: u64,
    pub constant_initializers_started: u64,
    pub static_initializers_started: u64,
    pub reachable_source_walks: u64,
    phase_nanoseconds: [u128; Phase::COUNT],
}

impl Work {
    /// Inclusive elapsed time in the requested phase, in instrumented builds only.
    pub fn phase_nanoseconds(&self, phase: Phase) -> u128 {
        self.phase_nanoseconds[phase as usize]
    }
}

/// Named analysis boundaries; nested scopes report inclusive elapsed time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    PrepareDeclarations,
    PrepareSignatures,
    BodyEnvironments,
    BodyCheck,
    OwnershipProjection,
    FullSnapshot,
    ReachableSources,
}

impl Phase {
    const COUNT: usize = 7;
}

pub(crate) enum Event {
    FullSnapshot,
    SelectedBody,
    SharedBody,
    BodyCheck,
    BodyRestore,
    SignatureCheck,
    ConstantInitializer,
    StaticInitializer,
    ReachableSources,
}

#[derive(Clone, Copy)]
struct Scope {
    serial: u64,
    work: Work,
}

thread_local! {
    static CURRENT: Cell<Option<Scope>> = const { Cell::new(None) };
    static SERIAL: Cell<u64> = const { Cell::new(0) };
}

/// A nested measurement would ambiguously attribute work to two operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlreadyMeasuring;

impl fmt::Display for AlreadyMeasuring {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a HIR work measurement is already active on this thread")
    }
}

impl Error for AlreadyMeasuring {}

/// Operation result and counters from this invocation, never from a cached result.
#[derive(Debug)]
pub struct Measurement<T> {
    pub value: T,
    pub work: Work,
}

struct Reset;

impl Drop for Reset {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(None));
    }
}

/// Measures a synchronous operation on the calling thread without allocating.
///
/// Nested measurement rejects before invoking its closure. An unwinding closure
/// clears the scope; ordinary returned errors retain the work performed before
/// failure. Work on other threads is outside this scope and is not counted.
pub fn measure<T>(operation: impl FnOnce() -> T) -> Result<Measurement<T>, AlreadyMeasuring> {
    if CURRENT.with(|current| current.get().is_some()) {
        return Err(AlreadyMeasuring);
    }
    let serial = SERIAL.with(|serial| {
        let next = serial
            .get()
            .checked_add(1)
            .expect("HIR work scope serial exhausted");
        serial.set(next);
        next
    });
    CURRENT.with(|current| {
        current.set(Some(Scope {
            serial,
            work: Work::default(),
        }))
    });
    let _reset = Reset;
    let value = operation();
    let work = CURRENT.with(|current| current.get().expect("active HIR work scope").work);
    Ok(Measurement { value, work })
}

pub(crate) fn record(event: Event) {
    if matches!(event, Event::BodyCheck) {
        milestone::body_check();
    }
    CURRENT.with(|current| {
        let Some(mut scope) = current.get() else {
            return;
        };
        let counter = match event {
            Event::FullSnapshot => &mut scope.work.full_snapshot_calls,
            Event::SelectedBody => &mut scope.work.selected_body_calls,
            Event::SharedBody => &mut scope.work.shared_body_hits,
            Event::BodyCheck => &mut scope.work.body_checks_started,
            Event::BodyRestore => &mut scope.work.bodies_restored,
            Event::SignatureCheck => &mut scope.work.signature_checks_started,
            Event::ConstantInitializer => &mut scope.work.constant_initializers_started,
            Event::StaticInitializer => &mut scope.work.static_initializers_started,
            Event::ReachableSources => &mut scope.work.reachable_source_walks,
        };
        *counter = counter.checked_add(1).expect("HIR work counter exhausted");
        current.set(Some(scope));
    });
}

/// Thread-confined phase timer; dropping outside its original scope has no effect.
pub struct PhaseGuard {
    active: Option<(u64, Instant)>,
    phase: Phase,
    _thread: PhantomData<Rc<()>>,
}

/// Starts an inclusive phase timer only when a measurement is active.
pub fn phase(phase: Phase) -> PhaseGuard {
    let active = CURRENT.with(|current| current.get().map(|scope| (scope.serial, Instant::now())));
    PhaseGuard {
        active,
        phase,
        _thread: PhantomData,
    }
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        let Some((serial, started)) = self.active else {
            return;
        };
        CURRENT.with(|current| {
            let Some(mut scope) = current.get().filter(|scope| scope.serial == serial) else {
                return;
            };
            let elapsed = &mut scope.work.phase_nanoseconds[self.phase as usize];
            *elapsed = elapsed
                .checked_add(started.elapsed().as_nanos())
                .expect("HIR phase duration exhausted");
            current.set(Some(scope));
        });
    }
}

#[cfg(test)]
mod tests;
