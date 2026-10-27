//! Collection orchestration over the heap's checked storage and root records.
use crate::{
    clock::Instant,
    error::RuntimeError,
    execution_metadata::{MetadataLive, MetadataTrace},
    gc::{GcCollection, GcHeap, HeapObjectId, collector, storage::append_value_edges},
    module::{ModuleKey, ModuleStore, collection::ProgramGraph},
    resource::HeapCharge,
    value::Value,
};

use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Node {
    Object(HeapObjectId),
    Program(ModuleKey),
}

struct MarkedGraph {
    nodes: Vec<Node>,
    metadata: MetadataLive,
}

impl GcHeap {
    pub(super) fn trace_values(&self, values: &[Value]) -> Option<Vec<HeapObjectId>> {
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let interfaces = self.interfaces.try_borrow().ok()?;
        let objects = self.objects.try_borrow().ok()?;
        let mut workspace = self.native_scan_workspace.try_borrow_mut().ok()?;
        let mut pending = Vec::new();
        let mut edges = values.iter().rev().copied().collect();
        append_value_edges(&mut edges, &mut pending);
        collector::mark(pending, |id, pending| {
            let object = self.object_ref(&objects, id)?;
            object.trace(&interfaces, &mut workspace, &mut |value| edges.push(value))?;
            if self.resources.is_quarantined() {
                return None;
            }
            append_value_edges(&mut edges, pending);
            Some(())
        })
    }

    /// Mark before detaching any storage. Native tracing/destruction cannot
    /// reenter execution, allocate or mutate the graph during this operation.
    pub(crate) fn collect(
        &self,
        modules: &ModuleStore,
        additional_roots: &[Value],
    ) -> Result<GcCollection, RuntimeError> {
        self.resources
            .collect_operation(|| self.collect_inner(modules, additional_roots))?
            .ok_or_else(|| {
                self.resources
                    .quarantine("invalid or borrowed storage in collection graph")
            })
    }

    // Only successful take creates these inert native identities. All public
    // publication/access validation still uses validate_value and rejects them.
    fn collection_value_valid(&self, value: &Value) -> bool {
        if let Value::GcHandle(id) = value
            && self
                .moved
                .try_borrow()
                .is_ok_and(|moved| moved.contains(id))
        {
            return true;
        }
        self.validate_value(value)
    }

    fn trace_graph(&self, programs: &ProgramGraph<'_>, values: &[Value]) -> Option<MarkedGraph> {
        if !values
            .iter()
            .all(|value| self.collection_value_valid(value))
        {
            return None;
        }
        let objects = self.objects.try_borrow().ok()?;
        let mut workspace = self.native_scan_workspace.try_borrow_mut().ok()?;
        let metadata_roots = self.metadata_snapshots()?;
        let groups = self.operation_groups.try_borrow().ok()?;
        let applications = self.applications.try_borrow().ok()?;
        let interfaces = self.interfaces.try_borrow().ok()?;
        let environments = self.environments.try_borrow().ok()?;
        let mut metadata = MetadataTrace::new(&environments, &groups, &applications, &interfaces);
        let mut pending = programs
            .roots()
            .into_iter()
            .map(Node::Program)
            .collect::<Vec<_>>();
        for root in &metadata_roots {
            metadata.programs(root.edge(), |owner| {
                pending.push(Node::Program(programs.executable_edge(owner)?));
                Some(())
            })?;
        }
        let mut edges = values.iter().rev().copied().collect();
        metadata.append_values(&mut edges);
        if !edges.iter().all(|value| self.collection_value_valid(value)) {
            return None;
        }
        let mut object_edges = Vec::new();
        append_value_edges(&mut edges, &mut object_edges);
        pending.extend(object_edges.drain(..).map(Node::Object));
        let live = collector::mark(pending, |node, pending| {
            match node {
                Node::Object(id) => {
                    if self.moved.try_borrow().ok()?.contains(&id) {
                        return Some(());
                    }
                    let object = self.object_ref(&objects, id)?;
                    object.trace(&interfaces, &mut workspace, &mut |value| edges.push(value))?;
                    if let Some(edge) = object.metadata() {
                        metadata.programs(edge, |owner| {
                            pending.push(Node::Program(programs.executable_edge(owner)?));
                            Some(())
                        })?;
                    }
                }
                Node::Program(id) => {
                    let mut applications = Vec::new();
                    programs.trace(id, &mut |value| edges.push(*value), &mut applications)?;
                    for edge in applications {
                        metadata.programs(edge, |owner| {
                            pending.push(Node::Program(programs.executable_edge(owner)?));
                            Some(())
                        })?;
                    }
                }
            }
            if self.resources.is_quarantined() {
                return None;
            }
            metadata.append_values(&mut edges);
            if !edges.iter().all(|value| self.collection_value_valid(value)) {
                return None;
            }
            append_value_edges(&mut edges, &mut object_edges);
            pending.extend(object_edges.drain(..).map(Node::Object));
            Some(())
        })?;
        Some(MarkedGraph {
            nodes: live,
            metadata: metadata.into_live_records(),
        })
    }

    fn collect_inner(
        &self,
        modules: &ModuleStore,
        additional_roots: &[Value],
    ) -> Option<GcCollection> {
        let started = Instant::now();
        let mut programs = modules.collection_graph()?;
        let mut values = self.root_snapshots()?;
        values.extend_from_slice(additional_roots);
        let mut live = HashSet::new();
        let mut live_programs = HashSet::new();
        let mut live_moved = HashSet::new();
        let graph = self.trace_graph(&programs, &values)?;
        for node in graph.nodes {
            match node {
                Node::Object(id) => {
                    if self.moved.try_borrow().ok()?.contains(&id) {
                        live_moved.insert(id);
                    } else {
                        live.insert(id);
                    }
                }
                Node::Program(id) => {
                    live_programs.insert(id);
                }
            }
        }
        // Read committed runtime charges before detaching. No provider size hooks.
        let (dead, dead_units, mut reclaimed_retained_bytes) = {
            let objects = self.objects.try_borrow().ok()?;
            let mut dead = Vec::new();
            let mut units = 0usize;
            let mut bytes = 0u128;
            for index in objects.occupied_directories() {
                let slot = objects.slots.get(index)?;
                let location = slot.location()?;
                let id = HeapObjectId::new(self.owner, index, slot.generation);
                if !live.contains(&id) {
                    let charge = objects
                        .get(location, &self.interfaces)?
                        .charge(&self.interfaces)?;
                    units = units.checked_add(charge.logical_units)?;
                    bytes = bytes.checked_add(charge.retained_bytes as u128)?;
                    dead.push(index);
                }
            }
            (dead, units, bytes)
        };
        if self.resources.is_quarantined() {
            return None;
        }
        let dead_programs = programs.prepare_sweep(&live_programs);
        let mut groups = self.operation_groups.try_borrow_mut().ok()?;
        let mut applications = self.applications.try_borrow_mut().ok()?;
        let mut interfaces = self.interfaces.try_borrow_mut().ok()?;
        let mut environments = self.environments.try_borrow_mut().ok()?;
        let mut moved = self.moved.try_borrow_mut().ok()?;
        let retired_identities = moved.len() - live_moved.len();
        let reclaimed_objects = dead.len();
        let reclaimed_units = dead_units.checked_add(retired_identities)?;

        // Temporary synchronous dead-list adapter; PG05 replaces this driver with
        // bounded sweep. Reserve all orchestration before detaching any identity.
        let retired = self
            .objects
            .try_borrow_mut()
            .ok()?
            .detach_many(self.owner, &dead)?;
        self.resources.release_heap_totals(
            reclaimed_units - retired_identities,
            reclaimed_retained_bytes,
        );
        drop(dead);
        self.objects.borrow_mut().retire_wrappers();
        moved.retain(|id| live_moved.contains(id));
        self.resources
            .release_heap_charge(HeapCharge::new(retired_identities, 0));
        drop(moved);
        let retired_groups = groups.detach(&graph.metadata.groups);
        let reclaimed_operation_groups = retired_groups.len();
        let retired_applications = applications.detach(&graph.metadata.applications);
        let reclaimed_method_applications = retired_applications.len();
        let retired_interfaces = interfaces.detach(&graph.metadata.interfaces);
        let reclaimed_interface_snapshots = retired_interfaces.len();
        let retired_environments = environments.detach(&graph.metadata.environments);
        let reclaimed_environments = retired_environments.len();
        drop(environments);
        drop(interfaces);
        drop(applications);
        drop(groups);
        let retired_modules = programs.detach(dead_programs);
        let reclaimed_modules = retired_modules.iter().map(|module| module.key).collect();
        drop(programs);

        self.roots.borrow_mut().drain_released();
        // Destructors run with no storage-table borrow. Finish disposal even when
        // one panics; the execution gate keeps the affected runtime quarantined.
        for payload in retired.payloads {
            reclaimed_retained_bytes += self.dispose_payload(payload) as u128;
        }
        for mut page in retired.pages {
            page.dispose(|| {
                self.resources
                    .quarantine("native payload destruction panicked");
            });
            let released = self.objects.borrow_mut().finish_page(&page);
            if page.release() {
                self.resources
                    .quarantine("representation owner destruction panicked");
            }
            self.resources
                .release_heap_charge(HeapCharge::new(0, released));
            reclaimed_retained_bytes += released as u128;
        }
        self.objects.borrow_mut().prune(live.len());
        for group in retired_groups {
            if catch_unwind(AssertUnwindSafe(|| drop(group))).is_err() {
                self.resources
                    .quarantine("operation group destruction panicked");
            }
        }
        for application in retired_applications {
            if catch_unwind(AssertUnwindSafe(|| drop(application))).is_err() {
                self.resources
                    .quarantine("method application destruction panicked");
            }
        }
        for interface in retired_interfaces {
            if catch_unwind(AssertUnwindSafe(|| drop(interface))).is_err() {
                self.resources
                    .quarantine("interface metadata destruction panicked");
            }
        }
        for environment in retired_environments {
            if catch_unwind(AssertUnwindSafe(|| drop(environment))).is_err() {
                self.resources
                    .quarantine("environment metadata destruction panicked");
            }
        }
        for module in retired_modules {
            if catch_unwind(AssertUnwindSafe(|| drop(module))).is_err() {
                self.resources
                    .quarantine("module metadata destruction panicked");
            }
        }
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
                .saturating_add(graph.metadata.groups.len())
                .saturating_add(graph.metadata.applications.len())
                .saturating_add(graph.metadata.interfaces.len())
                .saturating_add(graph.metadata.environments.len())
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
            live_objects: live.len(),
            pause,
        })
    }
}
