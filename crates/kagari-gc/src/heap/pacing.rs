//! Cooperative collection scheduling at an explicit, collection-safe boundary.

use crate::{
    error::GcError,
    heap::{Heap, Phase},
    root_source::RootScan,
    scan::WorkBudget,
    sweep::RETIREMENT_CAP,
};
use std::time::{Duration, Instant};

/// Thresholds for opt-in safepoint service, not memory quotas or pause guarantees.
/// Runtime/host code chooses safepoint frequency. No service occurs during access.
#[derive(Debug, Clone, Copy)]
pub struct PacingConfig {
    /// Minimum committed growth between cycle starts; must be nonzero.
    pub min_growth_bytes: usize,
    /// Next wakeup growth as a percentage of post-cycle managed bytes; nonzero.
    pub growth_percent: u16,
    /// Growth during an active cycle triggering a drain, in threshold multiples.
    /// Must be at least one; an idle burst uses at least two multiples so ordinary
    /// wakeup begins incrementally. Assists finish only one cycle.
    pub assist_multiplier: usize,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            min_growth_bytes: 16 * 1024,
            growth_percent: 50,
            assist_multiplier: 1,
        }
    }
}

/// One normal service call or a synchronous current-cycle drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomaticKind {
    Step,
    Assist,
}

/// Actual work performed at this safepoint, including disposal time.
#[derive(Debug, Clone, Copy)]
pub struct AutomaticReport {
    pub kind: AutomaticKind,
    pub work: usize,
    pub retired_owners: usize,
    pub completed_cycles: u64,
    pub elapsed: Duration,
    /// Idle metadata maintenance, included in elapsed, outside the scan budget.
    pub maintenance_elapsed: Duration,
}

#[derive(Debug)]
pub(super) struct Pacing {
    config: PacingConfig,
    threshold: usize,
}

impl Pacing {
    pub(super) fn new(config: PacingConfig) -> Result<Self, GcError> {
        if config.min_growth_bytes == 0
            || config.growth_percent == 0
            || config.assist_multiplier == 0
        {
            return Err(GcError::Limit("nonzero pacing thresholds required"));
        }
        Ok(Self {
            threshold: config.min_growth_bytes,
            config,
        })
    }

    pub(super) fn completed(&mut self, managed: usize) {
        let relative = (managed as u128 * u128::from(self.config.growth_percent) / 100)
            .min(usize::MAX as u128) as usize;
        self.threshold = relative.max(self.config.min_growth_bytes);
    }
}

impl Heap {
    /// Automatically select service from committed allocation growth and limits.
    /// Call between access scopes; retained results must already be rooted.
    ///
    /// Idle heaps sleep until growth reaches the configured threshold, a release
    /// batch is pending, or a configured admission limit is at least 75% occupied.
    /// An active cycle always advances even if allocation stops. Normal calls use
    /// an allowance of 1024 units and at most 32 detached owners. Declared
    /// indivisible providers may exceed work allowance. Excess growth or admission
    /// pressure drains one cycle through the same chunks; this is not time-bounded.
    /// Limits remain admission checks: service cannot rescue an allocation inside
    /// a scope or promise it fits afterward. None allocates nothing and reads no clock.
    /// A completing call also shrinks excess metadata buffers, separately timed
    /// in maintenance_elapsed. This suffix may copy live entries even for Step;
    /// use explicit step/trim_metadata to schedule maintenance independently.
    pub fn safepoint(&mut self) -> Result<Option<AutomaticReport>, GcError> {
        self.safepoint_with(&mut ())
    }

    pub(crate) fn safepoint_with(
        &mut self,
        source: &mut impl RootScan,
    ) -> Result<Option<AutomaticReport>, GcError> {
        self.ledger.begin(&self.physical);
        self.marking.check()?;
        let growth = self.ledger.allocation_growth;
        let releases = self.roots.pending_releases();
        let active = self.marking.phase != Phase::Idle;
        // Stable live data near a quota must not cause a full cycle at every idle
        // poll. New growth or pending releases can justify another pressure attempt.
        let pressure = (active || growth != 0 || releases != 0)
            && (near_limit(self.physical.backing_bytes, self.config.max_backing_bytes)
                || near_limit(self.ledger.indirect, self.config.max_indirect_bytes)
                || near_limit(self.physical.objects, self.config.max_objects)
                || near_limit(self.roots.count, self.config.max_roots));
        if !active && growth < self.pacing.threshold && releases < RETIREMENT_CAP && !pressure {
            return Ok(None);
        }
        let assist = pressure
            || growth
                >= self.pacing.threshold.saturating_mul(if active {
                    self.pacing.config.assist_multiplier
                } else {
                    self.pacing.config.assist_multiplier.max(2)
                });
        let kind = if assist {
            AutomaticKind::Assist
        } else {
            AutomaticKind::Step
        };
        let start = Instant::now();
        let cycles = self.marking.cycles;
        let report = self.service_work(WorkBudget::NORMAL, source)?;
        let mut work = report.work;
        let mut retired_owners = report.retired_owners;
        if assist {
            self.drain(&mut work, &mut retired_owners, source)?;
        }
        let maintenance_elapsed = self.trim_metadata()?.unwrap_or_default();
        Ok(Some(AutomaticReport {
            kind,
            work,
            retired_owners,
            completed_cycles: self.marking.cycles - cycles,
            elapsed: start.elapsed(),
            maintenance_elapsed,
        }))
    }
}

fn near_limit(used: usize, limit: usize) -> bool {
    limit != usize::MAX && used != 0 && used >= limit - limit / 4
}
