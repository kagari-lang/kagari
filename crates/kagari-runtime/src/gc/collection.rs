//! Collection orchestration over the heap's checked storage and root records.
use crate::{
    clock::Instant,
    error::RuntimeError,
    gc::{GcCollection, GcHeap, HeapObjectId, collector, storage::append_value_edges},
    host::HostRegistry,
    module::{ModuleStore, collection::ModuleSweep},
    native::storage::scan::{ScanBudget, ScanStatus},
    resource::HeapCharge,
    value::Value,
};

use std::panic::{AssertUnwindSafe, catch_unwind};

// One domain's batch is completely disposed before the next is detached.
const RETIRE_BATCH: usize = 32;

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

    /// Drain the sole marker synchronously. Each owner uses the same finite slice
    /// protocol exercised by test-only mutator interleaving; PG05 owns scheduling.
    pub(crate) fn collect(
        &self,
        modules: &ModuleStore,
        host: &HostRegistry,
    ) -> Result<GcCollection, RuntimeError> {
        let started = Instant::now();
        let mut programs = modules.begin_collection();
        let result = (|| {
            let mut roots = self.begin_initial_roots(modules, host)?;
            while self.scan_initial_root_slice(
                modules,
                host,
                &mut roots,
                &mut ScanBudget::new(usize::MAX),
            )? == ScanStatus::Pending
            {}
            self.drain_graph_work(modules)?;
            self.resources.collect_operation(|| {
                self.graph.try_lock()?.close_mark()?;
                self.sweep_closed(&mut programs, started).ok_or_else(|| {
                    RuntimeError::module_validation("invalid closed graph retirement")
                })
            })?
        })();
        if result.is_err() {
            if let Ok(mut graph) = self.graph.try_lock() {
                graph.fault();
            }
            return Err(self.resources.quarantine("collection graph failed"));
        }
        result
    }

    /// No store borrow spans destruction. A panic quarantines execution but does
    /// not abandon other detached records or leave their accounting outstanding.
    fn dispose_batches<T>(
        &self,
        mut detach: impl FnMut(&mut usize, usize) -> Vec<T>,
        release_credits: bool,
        panic_message: &'static str,
    ) -> usize {
        let mut cursor = 0;
        let mut count = 0;
        loop {
            let retired = detach(&mut cursor, RETIRE_BATCH);
            if retired.is_empty() {
                return count;
            }
            assert!(retired.len() <= RETIRE_BATCH);
            count += retired.len();
            {
                let mut graph = self.graph.lock();
                graph.observe_retirement(retired.len());
                if release_credits {
                    for _ in 0..retired.len() {
                        graph.retire_prepared();
                    }
                }
            }
            for record in retired {
                if catch_unwind(AssertUnwindSafe(|| drop(record))).is_err() {
                    self.resources.quarantine(panic_message);
                }
            }
        }
    }

    fn sweep_closed(
        &self,
        programs: &mut ModuleSweep<'_>,
        started: Instant,
    ) -> Option<GcCollection> {
        if self.resources.is_quarantined() {
            return None;
        }
        let mut dead_programs = programs.prepare_sweep()?;
        // Preflight every storage borrow before the first irreversible detach.
        let groups = self.operation_groups.try_borrow_mut().ok()?;
        let applications = self.applications.try_borrow_mut().ok()?;
        let interfaces = self.interfaces.try_borrow_mut().ok()?;
        let environments = self.environments.try_borrow_mut().ok()?;
        let native = self.native_applications.try_borrow_mut().ok()?;
        let calls = self.interface_calls.try_borrow_mut().ok()?;
        let descriptions = self.interface_descriptions.try_borrow_mut().ok()?;
        let mut objects = self.objects.try_borrow_mut().ok()?;
        let moved = self.moved.try_borrow_mut().ok()?;
        let retired_identities = {
            let graph = self.graph.lock();
            moved
                .values()
                .filter(|mark| !graph.is_survivor(*mark).expect("closed moved marks"))
                .count()
        };
        let counters = self.resources.counters();
        objects.begin_physical_sweep(
            &self.graph.lock(),
            counters
                .current_heap_units
                .checked_sub(retired_identities)?,
            counters.current_retained_bytes,
        )?;

        drop((
            groups,
            applications,
            interfaces,
            environments,
            native,
            calls,
            descriptions,
            objects,
            moved,
        ));
        self.graph.lock().begin_retirement();
        // The physical owner prepares/consumes charges once and retains progress.
        // Production still drains synchronously; PG05 owns driver interleaving.
        let mut released_backing = 0u128;
        loop {
            let batch = self.objects.borrow_mut().sweep_batch(
                self.owner,
                &self.interfaces,
                &mut self.graph.lock(),
                &mut ScanBudget::new(1024),
            )?;
            let complete = batch.complete;
            released_backing += self.dispose_sweep_batch(batch) as u128;
            if complete {
                break;
            }
        }
        let totals = self.objects.borrow_mut().finish_physical_sweep()?;
        let reclaimed_objects = totals.objects;
        let reclaimed_units = totals.units.checked_add(retired_identities)?;
        let reclaimed_retained_bytes = totals.bytes.checked_add(released_backing)?;
        let live_objects = totals.live;
        self.objects.borrow_mut().retire_wrappers();
        {
            let mut moved = self.moved.borrow_mut();
            let mut graph = self.graph.lock();
            moved.retain(|_, mark| {
                if graph.is_survivor(mark).expect("closed moved marks") {
                    return true;
                }
                graph.can_retire(mark).expect("closed moved work");
                graph.retire_prepared();
                false
            });
        }
        self.resources
            .release_heap_charge(HeapCharge::new(retired_identities, 0));
        let reclaimed_operation_groups = self.dispose_batches(
            |cursor, limit| {
                let graph = self.graph.lock();
                self.operation_groups
                    .borrow_mut()
                    .detach_batch(cursor, limit, |mark| {
                        graph.is_survivor(mark).expect("closed metadata marks")
                    })
            },
            true,
            "operation group destruction panicked",
        );
        let reclaimed_method_applications = self.dispose_batches(
            |cursor, limit| {
                let graph = self.graph.lock();
                self.applications
                    .borrow_mut()
                    .detach_batch(cursor, limit, |mark| {
                        graph.is_survivor(mark).expect("closed metadata marks")
                    })
            },
            true,
            "method application destruction panicked",
        );
        let reclaimed_interface_snapshots = self.dispose_batches(
            |cursor, limit| {
                let graph = self.graph.lock();
                self.interfaces
                    .borrow_mut()
                    .detach_batch(cursor, limit, |mark| {
                        graph.is_survivor(mark).expect("closed metadata marks")
                    })
            },
            true,
            "interface metadata destruction panicked",
        );
        let reclaimed_environments = self.dispose_batches(
            |cursor, limit| {
                let graph = self.graph.lock();
                self.environments
                    .borrow_mut()
                    .detach_batch(cursor, limit, |mark| {
                        graph.is_survivor(mark).expect("closed metadata marks")
                    })
            },
            true,
            "environment metadata destruction panicked",
        );
        let mut reclaimed_modules = Vec::new();
        self.dispose_batches(
            |_, limit| {
                let retired = programs.detach_program_batch(&mut dead_programs, limit);
                reclaimed_modules.extend(retired.iter().map(|record| record.key));
                retired
            },
            false,
            "module metadata destruction panicked",
        );
        self.dispose_batches(
            |_, limit| programs.detach_cell_batch(&mut dead_programs, limit),
            false,
            "static cell destruction panicked",
        );
        programs.finish(dead_programs);
        #[cfg(feature = "execution-diagnostics")]
        programs.record_storage_usage();
        self.roots.borrow_mut().drain_released();
        self.objects.borrow_mut().prune(live_objects);
        self.dispose_batches(
            |cursor, limit| {
                self.native_applications.borrow_mut().detach_batch(
                    cursor,
                    limit,
                    &mut self.graph.lock(),
                )
            },
            false,
            "native application destruction panicked",
        );
        self.dispose_batches(
            |cursor, limit| {
                self.interface_calls.borrow_mut().detach_batch(
                    cursor,
                    limit,
                    &mut self.graph.lock(),
                )
            },
            false,
            "interface call destruction panicked",
        );
        self.dispose_batches(
            |cursor, limit| {
                self.interface_descriptions.borrow_mut().detach_batch(
                    cursor,
                    limit,
                    &mut self.graph.lock(),
                )
            },
            false,
            "interface description destruction panicked",
        );
        if self.resources.is_quarantined() {
            return None;
        }
        self.graph.try_lock().ok()?.finish_sweep().ok()?;
        self.interfaces.borrow_mut().prune_descriptions();
        self.struct_layouts.borrow_mut().prune();
        self.native_operations.prune();
        let pause = started.elapsed();
        let mut stats = self.stats.borrow_mut();
        stats.collections += 1;
        stats.reclaimed_objects += reclaimed_objects;
        stats.allocated_objects -= reclaimed_objects;
        stats.last_pause = pause;
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
        Some(GcCollection {
            reclaimed_environments,
            reclaimed_operation_groups,
            reclaimed_method_applications,
            reclaimed_interface_snapshots,
            reclaimed_modules,
            reclaimed_objects,
            reclaimed_units,
            reclaimed_retained_bytes,
            live_objects,
            pause,
        })
    }
}
