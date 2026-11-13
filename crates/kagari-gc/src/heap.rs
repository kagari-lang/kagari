//! The exclusive heap driver, collection entrypoints and owned-memory observations.

pub mod accounting;
pub mod metadata;
pub mod pacing;
mod service;

use self::{
    accounting::{AccountingReport, MemoryLedger},
    pacing::{Pacing, PacingConfig},
    service::SweepStage,
};

use crate::{
    access::{Access, Scope},
    allocation::PhysicalHeap,
    descriptor::ScanWorkspace,
    error::GcError,
    marking::{ActiveScan, Marking, RootCursor},
    root::Roots,
    root_source::RootScan,
    scan::{ScanBudget, ScanKind, WorkBudget},
};
use std::{
    marker::PhantomData,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT_HEAP: AtomicU64 = AtomicU64::new(1);

/// Authoritative lifecycle state. Faulted heaps allow shutdown only.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Idle,
    Roots,
    Mark,
    Sweep,
    Faulted,
    Closed,
}

/// Managed admission limits and opt-in pacing, not a whole-process RSS quota.
#[derive(Debug, Clone, Copy)]
pub struct HeapConfig {
    pub pacing: PacingConfig,
    pub max_backing_bytes: usize,
    pub max_indirect_bytes: usize,
    pub max_objects: usize,
    pub max_roots: usize,
}

impl Default for HeapConfig {
    fn default() -> Self {
        Self {
            pacing: PacingConfig::default(),
            max_backing_bytes: usize::MAX,
            max_indirect_bytes: usize::MAX,
            max_objects: usize::MAX,
            max_roots: usize::MAX,
        }
    }
}

/// Logical managed observations; excludes Arc/table allocator overhead and RSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapStats {
    pub phase: Phase,
    pub objects: usize,
    /// Registered leases, including dropped roots awaiting release-notice service.
    pub root_registrations: usize,
    pub backing_bytes: usize,
    pub indirect_bytes: usize,
    /// Sum of committed provider-declared logical units, excluding object count.
    /// The collector assigns no language quota or session meaning to these units.
    pub provider_units: u128,
    pub scan_workspace_bytes: usize,
    pub peak_managed_bytes: usize,
    pub completed_cycles: u64,
    /// Committed shallow allocation and positive indirect growth since cycle start.
    /// This is pacing activity, not currently retained bytes; saturates on overflow.
    pub allocation_growth_bytes: usize,
}

/// One service call. Time includes its synchronous retirement/disposal suffix.
#[derive(Debug, Clone, Copy)]
pub struct StepReport {
    pub phase: Phase,
    pub work: usize,
    pub retired_owners: usize,
    pub completed_cycles: u64,
    pub elapsed: Duration,
    /// Minimum allowance needed by a pending bounded scanner when this call's
    /// allowance was too small. Retry with this budget; the heap is not faulted.
    pub required_budget: Option<usize>,
}

struct ServiceWork {
    work: usize,
    retired_owners: usize,
}

/// Full invocation, including finishing an already active cycle and one fresh one.
#[derive(Debug, Clone, Copy)]
pub struct CollectionReport {
    pub completed_cycles: u64,
    pub work: usize,
    pub retired_owners: usize,
    pub elapsed: Duration,
    /// Idle buffer maintenance, included in elapsed but not collector work units.
    pub maintenance_elapsed: Duration,
}

/// Destruction count, including objects that were rooted at shutdown.
#[derive(Debug, Clone, Copy)]
pub struct ShutdownReport {
    pub disposed_objects: usize,
    pub destructor_panics: usize,
}

/// A nonmoving managed heap with one driver and one suspended object scanner.
///
/// Collection runs through step/collect/safepoint, outside access intervals. Heap can
/// be transferred between threads; concurrent access is not supported. Drop safely
/// shuts down even with external handles, whose later destruction is inert.
#[derive(Debug)]
pub struct Heap {
    pub(crate) id: u64,
    pub(crate) config: HeapConfig,
    pub(crate) physical: PhysicalHeap,
    pub(crate) roots: Roots,
    pub(crate) marking: Marking,
    pub(crate) active: Option<ActiveScan>,
    pub(crate) workspace: ScanWorkspace,
    pub(crate) ledger: MemoryLedger,
    root_cursor: RootCursor,
    sweep_stage: SweepStage,
    pacing: Pacing,
}

impl Heap {
    pub fn new(config: HeapConfig) -> Result<Self, GcError> {
        let pacing = Pacing::new(config.pacing)?;
        let id = NEXT_HEAP
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| GcError::Limit("heap identity"))?;
        Ok(Self {
            id,
            config,
            physical: PhysicalHeap::default(),
            roots: Roots::new(id),
            marking: Marking::default(),
            active: None,
            workspace: ScanWorkspace::default(),
            ledger: MemoryLedger::default(),
            root_cursor: RootCursor::default(),
            sweep_stage: SweepStage::Physical,
            pacing,
        })
    }

    /// Begin a short non-reentrant interval; excludes service until it ends.
    pub fn access(&mut self) -> Result<Access<'_>, GcError> {
        self.ledger.begin(&self.physical);
        self.marking.check()?;
        Ok(Access {
            heap: self,
            non_transferable: PhantomData::<Rc<()>>,
        })
    }

    /// Run object operations with short loans and rootless, scope-bound handles.
    /// Collection and same-heap reentry are excluded until the callback returns.
    /// Return owned data or explicit Roots; Gc handles cannot escape this boundary.
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut heap = Heap::default();
    /// let root = heap.access().unwrap().alloc_leaf(1u64).unwrap();
    /// let escaped = heap.scope(|cx| cx.get(&root)).unwrap();
    /// println!("{escaped:?}");
    /// ```
    ///
    /// Nested scopes have distinct invariant brands; an outer handle cannot be
    /// used with the inner heap, even while both heaps remain alive.
    ///
    /// ```compile_fail
    /// use kagari_gc::heap::Heap;
    /// let mut outer = Heap::default();
    /// let mut inner = Heap::default();
    /// outer.scope(|cx| {
    ///     let value = cx.alloc_leaf(1u64)?;
    ///     inner.scope(|other| Ok(other.read(value, |value| *value)))
    /// }).unwrap();
    /// ```
    pub fn scope<R>(
        &mut self,
        operation: impl for<'gc> FnOnce(&mut Scope<'gc>) -> Result<R, GcError>,
    ) -> Result<R, GcError> {
        let mut scope = Scope {
            access: self.access()?,
            invariant: PhantomData,
        };
        operation(&mut scope)
    }

    /// Read the last access/service interval's committed occupancy and peaks.
    /// Reading does not reset it. A new access, step, collection or safepoint
    /// begins a new interval, including when admission subsequently fails.
    pub fn accounting(&self) -> AccountingReport {
        self.ledger.report(&self.physical)
    }

    pub fn stats(&self) -> HeapStats {
        HeapStats {
            phase: self.marking.phase,
            objects: self.physical.objects,
            root_registrations: self.roots.count,
            backing_bytes: self.physical.backing_bytes,
            indirect_bytes: self.ledger.indirect,
            provider_units: self.ledger.units,
            scan_workspace_bytes: self.workspace.bytes(),
            peak_managed_bytes: self.ledger.peak,
            completed_cycles: self.marking.cycles,
            allocation_growth_bytes: self.ledger.allocation_growth,
        }
    }

    /// Perform bounded cooperative work. Individual declared indivisible operations
    /// and Rust destructors may exceed the allowance; this is not a time bound.
    pub fn step(&mut self, allowance: WorkBudget) -> Result<StepReport, GcError> {
        self.step_with(allowance, &mut ())
    }

    pub(crate) fn step_with(
        &mut self,
        allowance: WorkBudget,
        source: &mut impl RootScan,
    ) -> Result<StepReport, GcError> {
        self.ledger.begin(&self.physical);
        let start = Instant::now();
        let cycles = self.marking.cycles;
        let work = self.service_work(allowance, source)?;
        Ok(StepReport {
            phase: self.marking.phase,
            work: work.work,
            retired_owners: work.retired_owners,
            completed_cycles: self.marking.cycles - cycles,
            elapsed: start.elapsed(),
            required_budget: self
                .active
                .as_ref()
                .and_then(|active| match active.descriptor.kind {
                    ScanKind::Bounded { max_work } if max_work * 5 > allowance.0 => {
                        Some((max_work * 5).max(8))
                    }
                    _ => None,
                }),
        })
    }

    // Shared untimed core: full collection measures the invocation once instead
    // of reading the clock for every bounded retirement batch.
    fn service_work(
        &mut self,
        allowance: WorkBudget,
        source: &mut impl RootScan,
    ) -> Result<ServiceWork, GcError> {
        self.marking.check()?;
        if allowance.0 < 8 {
            return Err(GcError::Limit("step allowance must be at least 8 units"));
        }
        let mut budget = ScanBudget::new(allowance.0);
        let cycles = self.marking.cycles;
        let result = catch_unwind(AssertUnwindSafe(|| self.service(&mut budget, source)));
        let retired_owners = match result {
            Ok(Ok(retired)) => retired,
            Ok(Err(error)) => {
                self.marking.phase = Phase::Faulted;
                return Err(error);
            }
            Err(_) => {
                self.marking.phase = Phase::Faulted;
                return Err(GcError::Provider("scanner or charge panicked"));
            }
        };
        if self.marking.cycles != cycles {
            self.pacing.completed(
                self.physical
                    .backing_bytes
                    .saturating_add(self.ledger.indirect),
            );
        }
        Ok(ServiceWork {
            work: budget.spent(),
            retired_owners,
        })
    }

    /// Finish a carried cycle, then collect fresh roots through the same engine.
    /// Includes idle metadata maintenance after all retirement has completed.
    pub fn collect(&mut self) -> Result<CollectionReport, GcError> {
        self.collect_with(&mut ())
    }

    pub(crate) fn collect_with(
        &mut self,
        source: &mut impl RootScan,
    ) -> Result<CollectionReport, GcError> {
        self.ledger.begin(&self.physical);
        self.marking.check()?;
        let start = Instant::now();
        let cycles = self.marking.cycles;
        let mut work = 0usize;
        let mut retired = 0usize;
        if self.marking.phase != Phase::Idle {
            self.drain(&mut work, &mut retired, source)?;
        }
        let report = self.service_work(WorkBudget::NORMAL, source)?;
        work = work
            .checked_add(report.work)
            .ok_or(GcError::Limit("collection work"))?;
        retired += report.retired_owners;
        self.drain(&mut work, &mut retired, source)?;
        let maintenance_elapsed = self.trim_metadata()?.expect("completed collection");
        Ok(CollectionReport {
            completed_cycles: self.marking.cycles - cycles,
            work,
            retired_owners: retired,
            elapsed: start.elapsed(),
            maintenance_elapsed,
        })
    }

    fn drain(
        &mut self,
        work: &mut usize,
        retired: &mut usize,
        source: &mut impl RootScan,
    ) -> Result<(), GcError> {
        while self.marking.phase != Phase::Idle {
            let report = self.service_work(WorkBudget::NORMAL, source)?;
            *work = work
                .checked_add(report.work)
                .ok_or(GcError::Limit("collection work"))?;
            *retired = retired
                .checked_add(report.retired_owners)
                .ok_or(GcError::Limit("retirement count"))?;
        }
        Ok(())
    }

    /// Permanently close admission and dispose every object outside storage loans.
    /// Destructor panics are contained and reported; later handle drops are inert.
    pub fn shutdown(&mut self) -> ShutdownReport {
        self.ledger.begin(&self.physical);
        self.marking.close();
        self.roots.disconnect();
        self.active = None;
        let (disposed_objects, destructor_panics) = self.dispose_all();
        self.roots.close();
        self.physical = PhysicalHeap::default();
        self.workspace = ScanWorkspace::default();
        self.ledger.indirect = 0;
        self.ledger.units = 0;
        self.ledger.allocation_growth = 0;
        ShutdownReport {
            disposed_objects,
            destructor_panics,
        }
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new(HeapConfig::default()).expect("process-local heap identity space")
    }
}

impl Drop for Heap {
    fn drop(&mut self) {
        self.shutdown();
    }
}
