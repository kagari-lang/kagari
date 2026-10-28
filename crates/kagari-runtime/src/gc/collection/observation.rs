//! Invocation and cycle timing segments; no mutator interval enters a cycle duration.
use crate::{clock::Instant, gc::GcHeap};
use std::time::Duration;

#[derive(Clone, Copy)]
pub(super) enum ServiceKind {
    Normal,
    PressureAssist,
    Explicit,
}

pub(super) struct Invocation {
    started: Instant,
    segment: Instant,
    pub(super) work: usize,
    pub(super) completed: Option<(usize, Duration)>,
}

impl Invocation {
    pub(super) fn new(heap: &GcHeap, entry: Option<Instant>) -> Self {
        if let Some(state) = &mut heap.stats.borrow_mut().observation {
            state.collecting = true;
        }
        let started = entry.unwrap_or_else(Instant::now);
        Self {
            started,
            segment: started,
            work: 0,
            completed: None,
        }
    }

    /// Finish the earlier cycle only when the next one starts, so inter-cycle
    /// orchestration is included instead of disappearing between step timers.
    pub(super) fn next_cycle(&mut self, heap: &GcHeap) {
        if let Some((work, previous)) = self.completed.take()
            && let Some(ended) = heap.collection_entry_time()
        {
            self.record_cycle(
                heap,
                work,
                previous.saturating_add(ended.duration_since(self.segment)),
            );
            self.segment = ended;
        }
    }

    pub(super) fn slice(&mut self, heap: &GcHeap, work: usize) {
        self.work = self.work.saturating_add(work);
        if let Some(state) = &mut heap.stats.borrow_mut().observation {
            let slices = &mut state.data.slices;
            slices.count = slices.count.saturating_add(1);
            slices.total_work = slices.total_work.saturating_add(work as u128);
            slices.last_work = work;
            slices.max_work = slices.max_work.max(work);
        }
    }

    fn record_cycle(&self, heap: &GcHeap, work: usize, time: Duration) {
        if let Some(state) = &mut heap.stats.borrow_mut().observation {
            let target = if state.carried {
                &mut state.data.carried_cycles
            } else {
                &mut state.data.cycles
            };
            target.record(work, time, false);
            state.carried = false;
        }
    }

    /// Called after result normalization, completion bookkeeping, cursor destruction
    /// and discarded automatic reports. Only metric publication follows this stamp.
    pub(super) fn finish(self, heap: &GcHeap, kind: ServiceKind, failed: bool) -> Duration {
        let ended = Instant::now();
        let elapsed = ended.duration_since(self.started);
        let segment = ended.duration_since(self.segment);
        if let Some((work, previous)) = self.completed {
            self.record_cycle(heap, work, previous.saturating_add(segment));
        } else if let Some(cycle) = heap.collection.borrow_mut().as_mut() {
            cycle.service_time = cycle.service_time.saturating_add(segment);
        }
        let mut stats = heap.stats.borrow_mut();
        if !failed {
            stats.last_pause = elapsed;
        }
        if let Some(state) = &mut stats.observation {
            let target = match kind {
                ServiceKind::Normal => &mut state.data.normal,
                ServiceKind::PressureAssist => &mut state.data.pressure_assist,
                ServiceKind::Explicit => &mut state.data.explicit,
            };
            target.record(self.work, elapsed, failed);
            state.collecting = false;
        }
        elapsed
    }
}
