//! Caller-owned fixed token storage; payload and final-owner Drop never hold heap borrows.
use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        directory::{
            pages::{Retired, batch::Disposal, retirement::RetiredRange},
            retirement::{FRAGMENT_FINISH_WORK, RETIRE_LIMIT, WORKSPACE_WORK},
        },
        metrics::{IndivisibleKind, IndivisibleMeasurements, IndivisibleSample, ObservationClock},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
    resource::ResourceState,
};

pub(in crate::gc) struct SweepBatch<'heap> {
    resources: &'heap ResourceState,
    pub(super) disposal: [Option<Disposal>; RETIRE_LIMIT],
    pub(super) fragments: usize,
    pub(super) objects: usize,
    pub(super) units: usize,
    pub(super) bytes: u128,
    pub(in crate::gc) status: ScanStatus,
}

impl<'heap> SweepBatch<'heap> {
    /// Caller reserves WORKSPACE_WORK before initializing any fixed workspace.
    #[inline(always)]
    pub(super) fn new(heap: &'heap GcHeap) -> Self {
        Self {
            resources: &heap.resources,
            disposal: [const { None }; RETIRE_LIMIT],
            fragments: 0,
            objects: 0,
            units: 0,
            bytes: 0,
            status: ScanStatus::Pending,
        }
    }

    pub(in crate::gc) fn has_retirements(&self) -> bool {
        self.objects != 0
    }
}

impl Drop for SweepBatch<'_> {
    fn drop(&mut self) {
        if self.objects == 0 && self.fragments == 0 {
            return;
        }
        // The caller declares this guard before the inner storage/graph guards.
        // An incomplete transaction is fail-closed, never silently made reusable.
        self.resources.quarantine("abandoned physical retirement");
        for token in &mut self.disposal[..self.fragments] {
            if let Some(mut token) = token.take() {
                token.dispose_unobserved(self.resources);
                if token.release() {
                    self.resources
                        .quarantine("representation owner destruction panicked");
                }
            }
        }
    }
}

impl Disposal {
    fn objects(&self) -> usize {
        match self {
            Self::Payload(_) => 1,
            Self::Range(range) => range.objects(),
        }
    }

    fn dispose_unobserved(&mut self, resources: &ResourceState) {
        match self {
            Self::Payload(payload) => {
                if payload.dispose() {
                    resources.quarantine("native payload destruction panicked");
                }
            }
            Self::Range(range) => {
                range.dispose(None, || {
                    resources.quarantine("native payload destruction panicked");
                });
            }
        }
    }

    fn release(self) -> bool {
        match self {
            Self::Payload(payload) => payload.release(),
            Self::Range(range) => range.release(),
        }
    }
}

#[derive(Default)]
pub(in crate::gc) struct SweepDisposal {
    pub(in crate::gc) objects: usize,
    pub(in crate::gc) units: usize,
    pub(in crate::gc) bytes: u128,
    #[cfg(test)]
    pub(in crate::gc) released_bytes: usize,
}

impl GcHeap {
    /// Stack workspace is initialized only after its allowance is reserved. It is
    /// borrowed by prepare/commit and disposal, never returned as a large value.
    pub(in crate::gc) fn retire_physical_batch(
        &self,
        budget: &mut ScanBudget,
    ) -> Result<Option<(ScanStatus, SweepDisposal)>, RuntimeError> {
        if !budget.take(WORKSPACE_WORK) {
            return Ok(None);
        }
        let mut batch = SweepBatch::new(self);
        {
            let mut objects = self.objects.try_borrow_mut().map_err(|_| {
                RuntimeError::module_validation("physical sweep storage is borrowed")
            })?;
            let mut graph = self.graph.try_lock()?;
            objects
                .sweep_batch(self.owner, &self.interfaces, &mut graph, budget, &mut batch)
                .ok_or_else(|| {
                    RuntimeError::module_validation("invalid physical retirement transaction")
                })?;
        }
        let status = batch.status;
        let disposal = self.dispose_sweep_batch(&mut batch)?;
        Ok(Some((status, disposal)))
    }

    #[inline(never)]
    fn dispose_observed_range(&self, range: &mut RetiredRange, clock: ObservationClock) {
        let mut measurements = IndivisibleMeasurements::default();
        range.dispose(Some((clock, &mut measurements)), || {
            self.resources
                .quarantine("native payload destruction panicked");
        });
        self.observe_indivisible_batch(IndivisibleKind::PayloadDrop, measurements);
    }

    fn dispose_observed_payload(&self, payload: &mut Retired, clock: ObservationClock) {
        let attribution = match payload {
            Retired::Payload { descriptor, .. } if descriptor.destroy.is_some() => Some((
                descriptor.kind.observation_owner(),
                descriptor.layout.size(),
            )),
            _ => None,
        };
        let started = attribution.and_then(|_| clock.start());
        let panicked = payload.dispose();
        if let (Some((owner, bytes)), Some(started)) = (attribution, started) {
            self.observe_indivisible(
                IndivisibleKind::PayloadDrop,
                IndivisibleSample::new(owner, 1, 1, Some(bytes), started.elapsed()),
            );
        }
        if panicked {
            self.resources
                .quarantine("native payload destruction panicked");
        }
    }

    /// Charges remain committed until every token's payload and final owner have
    /// been released. The outstanding transaction blocks another detach meanwhile.
    pub(in crate::gc) fn dispose_sweep_batch(
        &self,
        batch: &mut SweepBatch<'_>,
    ) -> Result<SweepDisposal, RuntimeError> {
        if !batch.has_retirements() {
            return Ok(SweepDisposal::default());
        }
        let observation = self.observation_clock();
        for token in batch.disposal[..batch.fragments].iter_mut().flatten() {
            if observation.is_enabled() {
                match token {
                    Disposal::Payload(payload) => {
                        self.dispose_observed_payload(payload, observation)
                    }
                    Disposal::Range(range) => self.dispose_observed_range(range, observation),
                }
            } else {
                token.dispose_unobserved(&self.resources);
            }
        }
        let finish_started = observation.start();
        let mut released = 0usize;
        let mut fragments = 0usize;
        {
            let mut objects = self.objects.borrow_mut();
            let mut graph = self.graph.try_lock()?;
            for token in batch.disposal[..batch.fragments].iter().flatten() {
                let bytes = objects.pages.finish_sweep_disposal(token, &mut graph)?;
                released = released.checked_add(bytes).expect("bounded total backing");
                fragments += 1;
            }
        }
        if released != 0
            && let Some(started) = finish_started
        {
            self.observe_indivisible(
                IndivisibleKind::BackingRelease,
                IndivisibleSample::new(
                    "physical page control backing",
                    batch.objects,
                    fragments * FRAGMENT_FINISH_WORK,
                    Some(released),
                    started.elapsed(),
                ),
            );
        }
        for token in &mut batch.disposal[..batch.fragments] {
            if let Some(token) = token.take() {
                let objects = token.objects();
                let started = observation.start();
                let panicked = token.release();
                if let Some(started) = started {
                    self.observe_indivisible(
                        IndivisibleKind::BackingRelease,
                        IndivisibleSample::new(
                            "retired payload backing/descriptor",
                            objects,
                            1,
                            None,
                            started.elapsed(),
                        ),
                    );
                }
                if panicked {
                    self.resources
                        .quarantine("representation owner destruction panicked");
                }
            }
        }
        let bytes = batch
            .bytes
            .checked_add(released as u128)
            .expect("prepared payload and backing bound");
        self.resources.release_heap_totals(batch.units, bytes);
        self.objects
            .borrow_mut()
            .sweep
            .as_mut()
            .expect("active physical sweep")
            .outstanding = false;
        let result = SweepDisposal {
            objects: batch.objects,
            units: batch.units,
            bytes,
            #[cfg(test)]
            released_bytes: released,
        };
        batch.objects = 0;
        batch.fragments = 0;
        Ok(result)
    }
}
