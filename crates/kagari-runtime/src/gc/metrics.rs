//! Bounded, opt-in collector observations available without execution instrumentation.
use crate::{
    clock::Instant,
    error::RuntimeError,
    gc::{GcHeap, graph::MarkPhase},
};
#[cfg(test)]
use std::cell::Cell;
use std::{any::TypeId, time::Duration};

/// Complete service invocations or completed cycles, never interleaved mutator time.
/// Counters and duration sums saturate rather than wrapping. Work is charged
/// structural units, not CPU instructions; a failed bounded scan may retain its
/// prepaid reservation when unwinding instead of settling unused work.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorkMeasurements {
    pub count: u64,
    pub failures: u64,
    pub total_work: u128,
    pub last_work: usize,
    pub max_work: usize,
    pub total_time: Duration,
    pub last_time: Duration,
    pub max_time: Duration,
}

impl WorkMeasurements {
    pub(crate) fn record(&mut self, work: usize, time: Duration, failed: bool) {
        self.count = self.count.saturating_add(1);
        self.failures = self.failures.saturating_add(u64::from(failed));
        self.total_work = self.total_work.saturating_add(work as u128);
        self.last_work = work;
        self.max_work = self.max_work.max(work);
        self.total_time = self.total_time.saturating_add(time);
        self.last_time = time;
        self.max_time = self.max_time.max(time);
    }
}

/// Structural slice accounting. Slices inside explicit calls/assists are not normal pauses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SliceMeasurements {
    pub count: u64,
    pub total_work: u128,
    pub last_work: usize,
    pub max_work: usize,
}

/// An opaque operation's semantic owner, not a graph identity or an exemption for a cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum IndivisibleKind {
    ProviderScan,
    PayloadDrop,
    BackingRelease,
    MetadataDrop,
    RootDrop,
    DescriptorDrop,
    WeakTokenDrop,
    EmptyMapRelease,
    QueueCompaction,
    LifecycleDrop,
}

impl IndivisibleKind {
    pub const ALL: [Self; 10] = [
        Self::ProviderScan,
        Self::PayloadDrop,
        Self::BackingRelease,
        Self::MetadataDrop,
        Self::RootDrop,
        Self::DescriptorDrop,
        Self::WeakTokenDrop,
        Self::EmptyMapRelease,
        Self::QueueCompaction,
        Self::LifecycleDrop,
    ];
}

/// One operation's attribution. Only the slowest sample per kind is retained.
/// `owner` is a static representation/store name; `rust_type` identifies a native
/// provider where available. Neither retains a script owner, descriptor or heap ID.
/// `known_bytes` is local backing or shallow representation size, not transitive
/// destructor cost/RSS or necessarily bytes freed; None explicitly means unavailable.
/// Physical payload drops name the representation kind, not the native Rust type.
/// Items and work are
/// the local operation's extent, not an estimate of arbitrary Rust destructor work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndivisibleSample {
    pub owner: &'static str,
    pub rust_type: Option<TypeId>,
    pub items: usize,
    pub work: usize,
    pub known_bytes: Option<usize>,
    pub time: Duration,
}

impl IndivisibleSample {
    pub(crate) fn new(
        owner: &'static str,
        items: usize,
        work: usize,
        known_bytes: Option<usize>,
        time: Duration,
    ) -> Self {
        Self {
            owner,
            rust_type: None,
            items,
            work,
            known_bytes,
            time,
        }
    }
}

/// Fixed-space accumulation of individual operations, including every range destructor.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IndivisibleMeasurements {
    pub count: u64,
    pub total_items: u128,
    pub total_work: u128,
    pub total_known_bytes: u128,
    pub unknown_size_count: u64,
    pub total_time: Duration,
    pub slowest: Option<IndivisibleSample>,
}

impl IndivisibleMeasurements {
    pub(crate) fn record(&mut self, sample: IndivisibleSample) {
        self.count = self.count.saturating_add(1);
        self.total_items = self.total_items.saturating_add(sample.items as u128);
        self.total_work = self.total_work.saturating_add(sample.work as u128);
        if let Some(bytes) = sample.known_bytes {
            self.total_known_bytes = self.total_known_bytes.saturating_add(bytes as u128);
        } else {
            self.unknown_size_count = self.unknown_size_count.saturating_add(1);
        }
        self.total_time = self.total_time.saturating_add(sample.time);
        if self.slowest.is_none_or(|old| sample.time > old.time) {
            self.slowest = Some(sample);
        }
    }

    pub(crate) fn merge(&mut self, other: Self) {
        self.count = self.count.saturating_add(other.count);
        self.total_items = self.total_items.saturating_add(other.total_items);
        self.total_work = self.total_work.saturating_add(other.total_work);
        self.total_known_bytes = self
            .total_known_bytes
            .saturating_add(other.total_known_bytes);
        self.unknown_size_count = self
            .unknown_size_count
            .saturating_add(other.unknown_size_count);
        self.total_time = self.total_time.saturating_add(other.total_time);
        if let Some(sample) = other.slowest
            && self.slowest.is_none_or(|old| sample.time > old.time)
        {
            self.slowest = Some(sample);
        }
    }
}

/// One host-selected observation window. Normal service, pressure assists and
/// explicit calls are disjoint; no-op/disabled safepoints and operations rejected
/// before collection admission are not collection samples.
/// Invocation timers include entry guards, completed-cycle bookkeeping and owned
/// cursor destruction. They stop immediately before publishing this measurement;
/// the observation update itself and return-to-caller instructions are excluded.
/// Indivisible durations are nested evidence, never subtracted from raw pauses.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CollectionObservation {
    pub normal: WorkMeasurements,
    pub pressure_assist: WorkMeasurements,
    pub explicit: WorkMeasurements,
    /// Full collector service for cycles begun in this window, including final bookkeeping.
    pub cycles: WorkMeasurements,
    /// At most one cycle already active at window start. Includes its accumulated
    /// pre-window collector timing, so it must not be attributed to this window's
    /// workload. Entry-guard timing is available only while observation is enabled.
    pub carried_cycles: WorkMeasurements,
    pub active_cycle_at_start: bool,
    pub slices: SliceMeasurements,
    /// Indexed by IndivisibleKind; use IndivisibleKind::ALL when reporting.
    pub indivisible: [IndivisibleMeasurements; IndivisibleKind::ALL.len()],
}

/// Stack-scoped opt-in for one operation or bounded batch. It contains no owner
/// borrow and is never stored in a continuation. Observation windows cannot start,
/// reset or finish during collector service, so callbacks cannot invalidate this
/// bounded batch's captured choice.
#[derive(Clone, Copy, Default)]
pub(crate) struct ObservationClock {
    enabled: bool,
}

impl ObservationClock {
    pub(in crate::gc) fn is_enabled(self) -> bool {
        self.enabled
    }

    pub(crate) fn start(self) -> Option<ObservationStart> {
        self.enabled.then(|| ObservationStart(observation_now()))
    }
}

pub(crate) struct ObservationStart(Instant);

impl ObservationStart {
    pub(crate) fn elapsed(self) -> Duration {
        observation_now().duration_since(self.0)
    }
}

#[cfg(test)]
thread_local! {
    static OBSERVATION_TIMER_READS: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(in crate::gc) fn observation_timer_reads() -> usize {
    OBSERVATION_TIMER_READS.get()
}

fn observation_now() -> Instant {
    #[cfg(test)]
    OBSERVATION_TIMER_READS.set(OBSERVATION_TIMER_READS.get() + 1);
    Instant::now()
}

#[derive(Debug, Default)]
pub(super) struct ObservationState {
    pub(super) data: CollectionObservation,
    pub(super) collecting: bool,
    pub(super) carried: bool,
}

impl GcHeap {
    /// Start or reset a bounded collector observation window in an ordinary build.
    /// The first start allocates one fixed-size buffer; reset reuses it. No service
    /// invocation allocates observation backing or invokes an observer callback.
    /// Rejects quarantined/borrow-excluded execution like other runtime operations.
    pub fn start_collection_observation(&self) -> Result<(), RuntimeError> {
        self.ensure_execution_allowed()?;
        let carried = self.graph.phase() != MarkPhase::Idle;
        let mut stats = self.stats.borrow_mut();
        if stats
            .observation
            .as_ref()
            .is_some_and(|state| state.collecting)
        {
            return Err(RuntimeError::module_validation(
                "cannot reset collection observation during service",
            ));
        }
        let state = stats.observation.get_or_insert_with(Default::default);
        **state = ObservationState {
            data: CollectionObservation {
                active_cycle_at_start: carried,
                ..Default::default()
            },
            carried,
            collecting: false,
        };
        Ok(())
    }

    /// Copy the current window without resetting maxima; available after a trace fault.
    pub fn collection_observation(&self) -> Option<CollectionObservation> {
        self.stats
            .borrow()
            .observation
            .as_ref()
            .map(|state| state.data)
    }

    /// End observation and release its fixed backing outside collector service.
    /// Returns None without changing the window if called during collector service
    /// (including a destructor callback), or when no window exists.
    pub fn finish_collection_observation(&self) -> Option<CollectionObservation> {
        let mut stats = self.stats.borrow_mut();
        if stats
            .observation
            .as_ref()
            .is_some_and(|state| state.collecting)
        {
            return None;
        }
        stats.observation.take().map(|state| state.data)
    }

    pub(crate) fn observation_clock(&self) -> ObservationClock {
        ObservationClock {
            enabled: self
                .stats
                .borrow()
                .observation
                .as_ref()
                .is_some_and(|state| state.collecting),
        }
    }

    pub(crate) fn collection_entry_time(&self) -> Option<Instant> {
        self.stats
            .borrow()
            .observation
            .as_ref()
            .map(|_| observation_now())
    }

    pub(crate) fn observe_indivisible(&self, kind: IndivisibleKind, sample: IndivisibleSample) {
        if let Some(state) = &mut self.stats.borrow_mut().observation
            && state.collecting
        {
            state.data.indivisible[kind as usize].record(sample);
        }
    }

    pub(in crate::gc) fn observe_indivisible_batch(
        &self,
        kind: IndivisibleKind,
        batch: IndivisibleMeasurements,
    ) {
        if let Some(state) = &mut self.stats.borrow_mut().observation
            && state.collecting
        {
            state.data.indivisible[kind as usize].merge(batch);
        }
    }
}
