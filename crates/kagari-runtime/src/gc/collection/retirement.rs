//! Owned sweep progress. Every detached batch is disposed before another owner runs.
use super::invalid;
use crate::{
    error::RuntimeError,
    execution_metadata::scanning::MetadataSweepCursor,
    gc::{
        GcCollection, GcHeap,
        directory::{
            descriptors::Descriptor,
            maintenance::{Maintenance, MaintenanceObservation, MaintenanceStep},
        },
        metrics::{IndivisibleKind, IndivisibleSample, ObservationClock},
        roots::RootReleaseCursor,
        weak_cache::{WEAK_CACHE_REMOVE_WORK, WeakCacheToken},
    },
    module::{ModuleStore, collection::ModuleSweep, layout_storage::StructStorageKey},
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::{
    any::type_name,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[cfg(test)]
mod tests;

const RETIRE_BATCH: usize = 32;
// Eleven heap stores below, plus the module/static store's exclusive admission.
pub(super) const SWEEP_PREFLIGHT_WORK: usize = 12;
// Caller-reserved bound for credit/generation/free-list commits, accounting and
// destruction dispatch. The owner charges inspected slots separately.
const RETIRE_WORK: usize = 8;

#[derive(Clone, Copy)]
enum Stage {
    Physical,
    Moved,
    Groups,
    Applications,
    Interfaces,
    Environments,
    Programs,
    Cells,
    NativeApplications,
    InterfaceCalls,
    InterfaceDescriptions,
    Roots,
    Storage,
    Finish,
}

pub(super) struct Retirement {
    stage: Stage,
    metadata: [MetadataSweepCursor; 7],
    programs: ModuleSweep,
    releases: RootReleaseCursor,
    storage: Maintenance,
    pending_structure: Option<(WeakCacheToken<StructStorageKey, Descriptor>, ScanStatus)>,
}

impl GcHeap {
    /// Under collection exclusion, prove every mutable sweep owner is available
    /// before any detach. No callback can intervene between these checks; all
    /// guards are released before the driver invokes a scanner or destructor.
    pub(super) fn preflight_sweep_access(&self, modules: &ModuleStore) -> Result<(), RuntimeError> {
        let guards = (
            self.objects.try_borrow_mut().map_err(|_| invalid())?,
            self.moved.try_borrow_mut().map_err(|_| invalid())?,
            self.roots.try_borrow_mut().map_err(|_| invalid())?,
            self.struct_layouts
                .try_borrow_mut()
                .map_err(|_| invalid())?,
            self.operation_groups
                .try_borrow_mut()
                .map_err(|_| invalid())?,
            self.applications.try_borrow_mut().map_err(|_| invalid())?,
            self.interfaces.try_borrow_mut().map_err(|_| invalid())?,
            self.environments.try_borrow_mut().map_err(|_| invalid())?,
            self.native_applications
                .try_borrow_mut()
                .map_err(|_| invalid())?,
            self.interface_calls
                .try_borrow_mut()
                .map_err(|_| invalid())?,
            self.interface_descriptions
                .try_borrow_mut()
                .map_err(|_| invalid())?,
        );
        modules.check_sweep_access()?;
        drop(guards);
        Ok(())
    }

    pub(super) fn begin_retirement(
        &self,
        modules: &ModuleStore,
    ) -> Result<Retirement, RuntimeError> {
        // The entry reservation includes SWEEP_PREFLIGHT_WORK before closure/setup.
        self.preflight_sweep_access(modules)?;
        let programs = modules.begin_sweep()?;
        let metadata = [
            self.operation_groups
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.applications
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.interfaces
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.environments
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.native_applications
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.interface_calls
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
            self.interface_descriptions
                .try_borrow()
                .map_err(|_| invalid())?
                .begin_sweep(),
        ];
        let releases = self
            .roots
            .try_borrow()
            .map_err(|_| invalid())?
            .begin_release_drain();
        let mut objects = self.objects.try_borrow_mut().map_err(|_| invalid())?;
        let mut moved = self.moved.try_borrow_mut().map_err(|_| invalid())?;
        let mut graph = self.graph.try_lock()?;
        let counters = self.resources.counters();
        objects
            .begin_physical_sweep(
                &graph,
                counters.current_heap_units,
                counters.current_retained_bytes,
            )
            .ok_or_else(invalid)?;
        moved.begin_sweep(counters.current_heap_units)?;
        let storage = objects.begin_maintenance();
        graph.begin_retirement();
        Ok(Retirement {
            stage: Stage::Physical,
            metadata,
            programs,
            releases,
            storage,
            pending_structure: None,
        })
    }

    pub(super) fn retirement_slice(
        &self,
        modules: &ModuleStore,
        cursor: &mut Retirement,
        budget: &mut ScanBudget,
        report: &mut GcCollection,
        report_modules: bool,
    ) -> Result<ScanStatus, RuntimeError> {
        // The driver preflights all stores on destructive admission or after a
        // mutator interval, not between bounded batches within this exclusion.
        loop {
            // Reserve task dispatch, including empty domains, before touching an owner.
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let before = budget.spent();
            let (status, next) = match cursor.stage {
                Stage::Physical => {
                    let Some((status, disposal)) = self.retire_physical_batch(budget)? else {
                        return Ok(ScanStatus::Pending);
                    };
                    if disposal.objects != 0 {
                        report.reclaimed_objects += disposal.objects;
                        report.reclaimed_units += disposal.units;
                        report.reclaimed_retained_bytes += disposal.bytes;
                        let mut stats = self.stats.borrow_mut();
                        stats.reclaimed_objects += disposal.objects;
                        stats.allocated_objects = stats
                            .allocated_objects
                            .checked_sub(disposal.objects)
                            .ok_or_else(invalid)?;
                    }
                    if status == ScanStatus::Complete {
                        let totals = self
                            .objects
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .finish_physical_sweep()
                            .ok_or_else(invalid)?;
                        // Taken inert identities are not physical live objects. Explicit
                        // collection always finishes a fresh, noninterleaved sweep last.
                        report.live_objects = totals.live;
                    }
                    (status, Stage::Moved)
                }
                Stage::Moved => {
                    let (status, units) = self
                        .moved
                        .try_borrow_mut()
                        .map_err(|_| invalid())?
                        .sweep_slice(&mut self.graph.try_lock()?, budget)?;
                    self.resources.release_heap_units(units);
                    report.reclaimed_units += units;
                    (status, Stage::Groups)
                }
                Stage::Groups => {
                    let limit = cursor.metadata[0].remaining_slots().min(RETIRE_BATCH);
                    let (status, count) = self.retire_slice(budget, limit, |budget, retired| {
                        self.operation_groups
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[0],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    report.reclaimed_operation_groups += count;
                    (status, Stage::Applications)
                }
                Stage::Applications => {
                    let limit = cursor.metadata[1].remaining_slots().min(RETIRE_BATCH);
                    let (status, count) = self.retire_slice(budget, limit, |budget, retired| {
                        self.applications
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[1],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    report.reclaimed_method_applications += count;
                    (status, Stage::Interfaces)
                }
                Stage::Interfaces => {
                    let limit = cursor.metadata[2].remaining_slots().min(RETIRE_BATCH);
                    let (status, count) = self.retire_slice(budget, limit, |budget, retired| {
                        self.interfaces
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[2],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    report.reclaimed_interface_snapshots += count;
                    (status, Stage::Environments)
                }
                Stage::Environments => {
                    let limit = cursor.metadata[3].remaining_slots().min(RETIRE_BATCH);
                    let (status, count) = self.retire_slice(budget, limit, |budget, retired| {
                        self.environments
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[3],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    report.reclaimed_environments += count;
                    (status, Stage::Programs)
                }
                Stage::Programs => {
                    let (status, _) =
                        self.retire_slice(budget, RETIRE_BATCH, |budget, retired| {
                            modules.sweep_programs(
                                &mut cursor.programs,
                                budget,
                                RETIRE_BATCH,
                                report_modules.then_some(&mut report.reclaimed_modules),
                                retired,
                            )
                        })?;
                    (status, Stage::Cells)
                }
                Stage::Cells => {
                    let (status, _) =
                        self.retire_slice(budget, RETIRE_BATCH, |budget, retired| {
                            modules.sweep_cells(&mut cursor.programs, budget, RETIRE_BATCH, retired)
                        })?;
                    (status, Stage::NativeApplications)
                }
                Stage::NativeApplications => {
                    let limit = cursor.metadata[4].remaining_slots().min(RETIRE_BATCH);
                    let (status, _) = self.retire_slice(budget, limit, |budget, retired| {
                        self.native_applications
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[4],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    (status, Stage::InterfaceCalls)
                }
                Stage::InterfaceCalls => {
                    let limit = cursor.metadata[5].remaining_slots().min(RETIRE_BATCH);
                    let (status, _) = self.retire_slice(budget, limit, |budget, retired| {
                        self.interface_calls
                            .try_borrow_mut()
                            .map_err(|_| invalid())?
                            .detach_slice(
                                &mut cursor.metadata[5],
                                budget,
                                limit,
                                &mut self.graph.try_lock()?,
                                retired,
                            )
                    })?;
                    (status, Stage::InterfaceDescriptions)
                }
                Stage::InterfaceDescriptions => {
                    let limit = cursor.metadata[6].remaining_slots().min(RETIRE_BATCH);
                    let (status, _) = self.retire_with_cache_slice(
                        budget,
                        limit,
                        WEAK_CACHE_REMOVE_WORK,
                        |budget, cache, retired| {
                            let status = self
                                .interface_descriptions
                                .try_borrow_mut()
                                .map_err(|_| invalid())?
                                .detach_slice(
                                    &mut cursor.metadata[6],
                                    budget,
                                    limit,
                                    &mut self.graph.try_lock()?,
                                    retired,
                                );
                            for record in retired.iter() {
                                if self.release_interface_description_cache(record, cache)?
                                    != ScanStatus::Complete
                                {
                                    // Every detached record has its own pre-reserved release.
                                    // Exhaustion here is an internal bound violation, not a yield.
                                    return Err(invalid());
                                }
                            }
                            status
                        },
                    )?;
                    (status, Stage::Roots)
                }
                Stage::Roots => (
                    self.drain_root_release_slice(&mut cursor.releases, budget)?,
                    Stage::Storage,
                ),
                Stage::Storage => (self.storage_slice(cursor, budget)?, Stage::Finish),
                Stage::Finish => {
                    #[cfg(feature = "execution-diagnostics")]
                    {
                        if !budget.take(1) {
                            return Ok(ScanStatus::Pending);
                        }
                        // Constant-time Vec len/capacity facts, not a module traversal.
                        modules.record_collection_storage_usage();
                    }
                    // Queue shrinking is an allocator operation, not a hidden graph
                    // traversal; its opaque duration is reported separately.
                    let mut graph = self.graph.try_lock()?;
                    let started = self.observation_clock().start();
                    let compacted = graph.finish_sweep()?;
                    if compacted != 0
                        && let Some(started) = started
                    {
                        self.observe_indivisible(
                            IndivisibleKind::QueueCompaction,
                            IndivisibleSample::new(
                                "graph pending buffer",
                                0,
                                1,
                                Some(compacted),
                                started.elapsed(),
                            ),
                        );
                    }
                    return Ok(ScanStatus::Complete);
                }
            };
            if self.resources.is_quarantined() {
                return Err(invalid());
            }
            if status == ScanStatus::Complete {
                cursor.stage = next;
            }
            // A partial owner may need a larger indivisible retirement reservation
            // than this invocation has left. Do not spin or replenish its budget.
            if (status == ScanStatus::Pending && budget.spent() == before)
                || budget.remaining() == 0
            {
                return Ok(ScanStatus::Pending);
            }
        }
    }

    fn retire_slice<T>(
        &self,
        budget: &mut ScanBudget,
        limit: usize,
        detach: impl FnOnce(&mut ScanBudget, &mut Vec<T>) -> Result<ScanStatus, RuntimeError>,
    ) -> Result<(ScanStatus, usize), RuntimeError> {
        self.retire_with_cache_slice(budget, limit, 0, |scanning, _, retired| {
            detach(scanning, retired)
        })
    }

    fn retire_with_cache_slice<T>(
        &self,
        budget: &mut ScanBudget,
        limit: usize,
        cache_work: usize,
        detach: impl FnOnce(
            &mut ScanBudget,
            &mut ScanBudget,
            &mut Vec<T>,
        ) -> Result<ScanStatus, RuntimeError>,
    ) -> Result<(ScanStatus, usize), RuntimeError> {
        assert!(limit <= RETIRE_BATCH, "bounded retirement reservation");
        let available = budget.remaining();
        let reserved = limit * (RETIRE_WORK + cache_work);
        if limit != 0 && available <= reserved {
            return Ok((ScanStatus::Pending, 0));
        }
        // Both children are backed by this invocation's allowance before detach.
        // Slot inspection cannot spend the fixed retirement/cache-release reserve.
        assert!(budget.take(available));
        let mut scanning = ScanBudget::new(available - reserved);
        let mut cache = ScanBudget::new(limit * cache_work);
        let mut retired = Vec::new();
        let result = catch_unwind(AssertUnwindSafe(|| {
            detach(&mut scanning, &mut cache, &mut retired)
        }))
        .unwrap_or_else(|_| {
            Err(self
                .resources
                .quarantine("collection metadata retirement panicked"))
        });
        let count = retired.len();
        assert!(count <= limit, "reserved detached owners");
        budget.refund(available);
        budget.charge(scanning.spent() + cache.spent() + count * RETIRE_WORK);
        let observed = self
            .graph
            .try_lock()
            .map(|mut graph| graph.observe_retirement(count));
        let observation = self.observation_clock();
        for record in retired {
            let started = observation.start();
            if catch_unwind(AssertUnwindSafe(|| drop(record))).is_err() {
                self.resources
                    .quarantine("collection metadata destruction panicked");
            }
            if let Some(started) = started {
                self.observe_indivisible(
                    IndivisibleKind::MetadataDrop,
                    IndivisibleSample::new(
                        type_name::<T>(),
                        1,
                        RETIRE_WORK,
                        Some(size_of::<T>()),
                        started.elapsed(),
                    ),
                );
            }
        }
        if self.resources.is_quarantined() {
            return Err(invalid());
        }
        observed?;
        result.map(|status| (status, count))
    }

    // Only enabled observation enters this frame; no accumulator is initialized or
    // returned through disabled maintenance, and metric merging is outside borrows.
    #[inline(never)]
    fn observed_storage_maintenance(
        &self,
        cursor: &mut Maintenance,
        budget: &mut ScanBudget,
        clock: ObservationClock,
    ) -> Result<MaintenanceStep, RuntimeError> {
        let mut observation = MaintenanceObservation::new(clock);
        let step = self
            .objects
            .try_borrow_mut()
            .map_err(|_| invalid())?
            .maintenance_slice(cursor, budget, Some(&mut observation));
        self.observe_indivisible_batch(IndivisibleKind::DescriptorDrop, observation.destruction);
        if let Some((kind, sample)) = observation.allocation {
            self.observe_indivisible(kind, sample);
        }
        Ok(step)
    }

    fn storage_slice(
        &self,
        cursor: &mut Retirement,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        let reserved = RETIRE_WORK + WEAK_CACHE_REMOVE_WORK;
        let available = budget.remaining();
        if available < reserved {
            return Ok(ScanStatus::Pending);
        }
        assert!(budget.take(available));
        let mut scanning = ScanBudget::new(available - reserved);
        let mut release = ScanBudget::new(WEAK_CACHE_REMOVE_WORK);
        if cursor.pending_structure.is_none() {
            let observation = self.observation_clock();
            let step = if observation.is_enabled() {
                self.observed_storage_maintenance(&mut cursor.storage, &mut scanning, observation)?
            } else {
                self.objects
                    .try_borrow_mut()
                    .map_err(|_| invalid())?
                    .maintenance_slice(&mut cursor.storage, &mut scanning, None)
            };
            let Some(token) = step.removed_structure else {
                budget.refund(available);
                budget.charge(scanning.spent());
                return Ok(step.status);
            };
            cursor.pending_structure = Some((token, step.status));
        }
        let (token, _) = cursor
            .pending_structure
            .as_ref()
            .expect("owned release token");
        let (status, removed) = self
            .struct_layouts
            .try_borrow_mut()
            .map_err(|_| invalid())?
            .release(token, &mut release);
        budget.refund(available);
        budget.charge(scanning.spent() + release.spent());
        if status == ScanStatus::Pending {
            // Do not lose a detached descriptor's cleanup association on a yield.
            // The cache API promises no removal or debit on insufficient budget.
            debug_assert!(removed.is_none());
            return Ok(ScanStatus::Pending);
        }
        // Destruction dispatch was included in the reservation before detachment.
        assert!(budget.take(RETIRE_WORK));
        let (token, status) = cursor
            .pending_structure
            .take()
            .expect("completed release token");
        let items = 1 + usize::from(removed.is_some());
        let started = self.observation_clock().start();
        drop((token, removed));
        if let Some(started) = started {
            self.observe_indivisible(
                IndivisibleKind::WeakTokenDrop,
                IndivisibleSample::new(
                    "StructStorageKey cache tokens",
                    items,
                    RETIRE_WORK,
                    None,
                    started.elapsed(),
                ),
            );
        }
        Ok(status)
    }
}
