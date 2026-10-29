//! One persistent lifecycle, driven only at legal runtime collection boundaries.
mod observation;
mod retirement;
#[cfg(test)]
mod tests;

use crate::{
    clock::Instant,
    error::RuntimeError,
    gc::{
        GcCollection, GcHeap, HeapObjectId, collector,
        graph::{MarkPhase, roots::InitialRoots},
        metrics::{IndivisibleKind, IndivisibleSample},
        storage::append_value_edges,
    },
    host::HostRegistry,
    module::{ModuleStore, collection::AbandonmentTicket},
    native::storage::scan::{NORMAL_WORK, ScanBudget, ScanStatus},
    resource::CommittedPressure,
    value::Value,
};
use observation::{Invocation, ServiceKind};
use retirement::{Retirement, SWEEP_PREFLIGHT_WORK};
use std::{fmt, time::Duration};

/// Cursors and accounting only: GraphState owns the authoritative cycle phase.
/// No borrow, native loan, marker guard or borrowed program survives a service call.
pub(super) struct CollectionCycle {
    roots: InitialRoots,
    retirement: Option<Retirement>,
    abandonment: AbandonmentTicket,
    pressure: CommittedPressure,
    metadata_pressure: u128,
    unit_threshold: u128,
    byte_threshold: u128,
    work: usize,
    service_time: Duration,
}

impl fmt::Debug for CollectionCycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CollectionCycle")
            .field("roots", &self.roots)
            .field("work", &self.work)
            .field("service_time", &self.service_time)
            .finish_non_exhaustive()
    }
}

impl GcHeap {
    pub(super) fn trace_values(&self, values: &[Value]) -> Option<Vec<HeapObjectId>> {
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let interfaces = self.interfaces.try_borrow().ok()?;
        let objects = self.objects.try_borrow().ok()?;
        let mut pending = Vec::new();
        let mut edges = values.iter().rev().copied().collect();
        append_value_edges(&mut edges, &mut pending);
        collector::mark(pending, |id, pending| {
            let object = self.object_ref(&objects, id)?;
            object.inspect(&interfaces, &mut |value| edges.push(value))?;
            if self.resources.is_quarantined() {
                return None;
            }
            append_value_edges(&mut edges, pending);
            Some(())
        })
    }

    /// Finish an active cycle, then collect the current roots in one fresh cycle.
    /// Reports are invocation-local; earlier automatic retirements are never replayed.
    pub(crate) fn collect(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        entry: Option<Instant>,
    ) -> Result<GcCollection, RuntimeError> {
        let mut invocation = Invocation::new(self, entry);
        let mut report = GcCollection::default();
        let result = (|| {
            if self.graph.phase() != MarkPhase::Idle {
                self.finish_cycle(modules, host, &mut report, true, &mut invocation)?;
            }
            self.finish_cycle(modules, host, &mut report, true, &mut invocation)
        })();
        let result = self.collection_result(result);
        match result {
            Ok(()) => {
                report.pause = invocation.finish(self, ServiceKind::Explicit, false);
                Ok(report)
            }
            Err(error) => {
                drop(report);
                invocation.finish(self, ServiceKind::Explicit, true);
                Err(error)
            }
        }
    }

    /// Automatic work is disabled as a whole, including continuation and assists.
    /// Occupancy is only a start trigger; it cannot stop an already active cycle.
    pub(crate) fn service_collection(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        entry: Option<Instant>,
    ) -> Result<(), RuntimeError> {
        if !self.automatic_collection_enabled() {
            return Ok(());
        }
        let active = self.graph.phase() != MarkPhase::Idle;
        if !active && !self.collection_due() {
            modules.check_collection_access()?;
            if !modules.abandonment_pending() {
                return Ok(());
            }
        }
        self.resources.ensure_not_publishing()?;
        self.ensure_no_storage_borrow()?;
        self.resources.ensure_execution_allowed()?;
        let mut invocation = Invocation::new(self, entry);
        let mut kind = ServiceKind::Normal;
        let result = (|| {
            let assist = if active {
                let cycle = self.collection.try_borrow().map_err(|_| invalid())?;
                let cycle = cycle.as_ref().ok_or_else(invalid)?;
                let pressure = self.resources.committed_pressure();
                let metadata = self.graph.try_lock()?.committed_metadata_pressure();
                pressure
                    .units
                    .saturating_sub(cycle.pressure.units)
                    .saturating_add(metadata.saturating_sub(cycle.metadata_pressure))
                    >= cycle.unit_threshold
                    || pressure.bytes.saturating_sub(cycle.pressure.bytes) >= cycle.byte_threshold
            } else {
                false
            };
            let mut discarded = GcCollection::default();
            if assist {
                kind = ServiceKind::PressureAssist;
                self.finish_cycle(modules, host, &mut discarded, false, &mut invocation)
            } else {
                self.collection_step(
                    modules,
                    host,
                    &mut discarded,
                    false,
                    &mut invocation,
                    NORMAL_WORK,
                )
                .map(drop)
            }
        })();
        let result = self.collection_result(result);
        invocation.finish(self, kind, result.is_err());
        result
    }

    fn finish_cycle(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        report: &mut GcCollection,
        report_modules: bool,
        invocation: &mut Invocation,
    ) -> Result<(), RuntimeError> {
        // Drain the same owner/chunk engine without imposing a mutator-service yield.
        // Provider chunks and detached-owner bounds remain independent of this allowance.
        while self.collection_step(
            modules,
            host,
            report,
            report_modules,
            invocation,
            usize::MAX,
        )? == ScanStatus::Pending
        {}
        Ok(())
    }

    fn start_cycle(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
    ) -> Result<Box<CollectionCycle>, RuntimeError> {
        let abandonment = modules.begin_collection();
        let roots = self.begin_initial_roots(modules, host)?;
        Ok(Box::new(CollectionCycle {
            roots,
            retirement: None,
            abandonment,
            pressure: self.resources.committed_pressure(),
            metadata_pressure: self.graph.try_lock()?.committed_metadata_pressure(),
            unit_threshold: self.next_collection.get() as u128,
            byte_threshold: self.next_byte_collection.get(),
            work: 0,
            service_time: Duration::ZERO,
        }))
    }

    fn collection_step(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        report: &mut GcCollection,
        report_modules: bool,
        invocation: &mut Invocation,
        allowance: usize,
    ) -> Result<ScanStatus, RuntimeError> {
        if self.graph.phase() == MarkPhase::Idle {
            invocation.next_cycle(self);
        }
        let mut budget = ScanBudget::new(allowance);
        let mut cycle = if self.graph.phase() == MarkPhase::Idle {
            // Four root cursor captures, lifecycle setup and pressure snapshot.
            assert!(budget.take(6));
            match self.start_cycle(modules, host) {
                Ok(cycle) => cycle,
                Err(error) => {
                    invocation.slice(self, budget.spent());
                    return Err(error);
                }
            }
        } else {
            self.collection
                .try_borrow_mut()
                .map_err(|_| invalid())?
                .take()
                .ok_or_else(invalid)?
        };
        let result = self.advance_cycle(
            modules,
            host,
            &mut cycle,
            &mut budget,
            report,
            report_modules,
        );
        invocation.slice(self, budget.spent());
        cycle.work = cycle.work.saturating_add(budget.spent());
        match result {
            Ok(ScanStatus::Complete) => {
                invocation.completed = Some((cycle.work, cycle.service_time));
                self.complete_cycle(*cycle);
            }
            _ => *self.collection.try_borrow_mut().map_err(|_| invalid())? = Some(cycle),
        }
        result
    }

    fn advance_cycle(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
        cycle: &mut CollectionCycle,
        budget: &mut ScanBudget,
        report: &mut GcCollection,
        report_modules: bool,
    ) -> Result<ScanStatus, RuntimeError> {
        if self.graph.phase() == MarkPhase::Sweep {
            if !budget.take(SWEEP_PREFLIGHT_WORK) {
                return Ok(ScanStatus::Pending);
            }
            // A mutator interval may have acquired a view into any later store.
            // Fresh mark-to-sweep admission below already performs this preflight.
            self.resources
                .collect_operation(|| self.preflight_sweep_access(modules))??;
        }
        loop {
            match self.graph.phase() {
                MarkPhase::Roots => {
                    if self.scan_initial_root_slice(modules, host, &mut cycle.roots, budget)?
                        == ScanStatus::Pending
                    {
                        return Ok(ScanStatus::Pending);
                    }
                }
                MarkPhase::Mark => {
                    if self.scan_graph_slice(modules, budget)? == ScanStatus::Pending {
                        return Ok(ScanStatus::Pending);
                    }
                    // Reserve closure, finite cursor captures and all-owner exclusive
                    // admission before setting up any destructive phase state.
                    if !budget.take(16 + SWEEP_PREFLIGHT_WORK) {
                        return Ok(ScanStatus::Pending);
                    }
                    self.resources
                        .collect_operation(|| {
                            self.graph.try_lock()?.close_mark()?;
                            self.begin_retirement(modules)
                        })?
                        .map(|retirement| cycle.retirement = Some(retirement))?;
                }
                MarkPhase::Sweep => {
                    return self.resources.collect_operation(|| {
                        self.retirement_slice(
                            modules,
                            cycle.retirement.as_mut().ok_or_else(invalid)?,
                            budget,
                            report,
                            report_modules,
                        )
                    })?;
                }
                MarkPhase::Idle | MarkPhase::Faulted | MarkPhase::Poisoned => return Err(invalid()),
            }
        }
    }

    fn complete_cycle(&self, cycle: CollectionCycle) {
        debug_assert_eq!(self.graph.phase(), MarkPhase::Idle);
        let started = self.observation_clock().start();
        cycle.abandonment.finish();
        drop((cycle.roots, cycle.retirement));
        if let Some(started) = started {
            self.observe_indivisible(
                IndivisibleKind::LifecycleDrop,
                IndivisibleSample::new(
                    "CollectionCycle",
                    1,
                    1,
                    Some(size_of::<CollectionCycle>()),
                    started.elapsed(),
                ),
            );
        }
        let mut stats = self.stats.borrow_mut();
        stats.collections += 1;

        self.next_collection.set(
            self.resources
                .counters()
                .current_heap_units
                .saturating_add(self.operation_groups.borrow().count())
                .saturating_add(self.applications.borrow().count())
                .saturating_add(self.interfaces.borrow().count())
                .saturating_add(self.environments.borrow().count())
                .saturating_mul(2)
                .max(self.config.collection_threshold.unwrap_or(usize::MAX))
                .max(1),
        );
        self.next_byte_collection.set(
            self.resources
                .counters()
                .current_retained_bytes
                .saturating_mul(2)
                .max(self.config.retained_byte_threshold.max(1) as u128),
        );
    }

    fn collection_result<T>(&self, result: Result<T, RuntimeError>) -> Result<T, RuntimeError> {
        if result.is_err() {
            if let Ok(mut graph) = self.graph.try_lock() {
                graph.fault();
            }
            return Err(self.resources.quarantine("collection graph failed"));
        }
        result
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed collection lifecycle")
}
