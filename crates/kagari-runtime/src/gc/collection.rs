//! Collection orchestration over the heap's checked storage and root records.
use crate::{
    error::RuntimeError,
    execution_metadata::{MetadataLive, MetadataTrace},
    gc::{GcCollection, GcHeap, HeapObjectId, collector, storage::append_value_edges},
    module::{ModuleKey, ModuleStore, collection::ProgramGraph},
    value::Value,
};

use std::{
    collections::HashSet,
    panic::{AssertUnwindSafe, catch_unwind},
    time::Instant,
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
        let mut pending = Vec::new();
        let mut edges = values.iter().rev().collect();
        append_value_edges(&mut edges, &mut pending);
        collector::mark(pending, |id, pending| {
            let object = self.object_ref(&objects, id)?;
            object.trace(&interfaces, &mut |value| edges.push(value))?;
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

    fn trace_graph(&self, programs: &ProgramGraph<'_>, values: &[Value]) -> Option<MarkedGraph> {
        if !values.iter().all(|value| self.validate_value(value)) {
            return None;
        }
        let objects = self.objects.try_borrow().ok()?;
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
        let mut edges = values.iter().rev().collect();
        metadata.append_values(&mut edges);
        let mut object_edges = Vec::new();
        append_value_edges(&mut edges, &mut object_edges);
        pending.extend(object_edges.drain(..).map(Node::Object));
        let live = collector::mark(pending, |node, pending| {
            match node {
                Node::Object(id) => {
                    let object = self.object_ref(&objects, id)?;
                    object.trace(&interfaces, &mut |value| edges.push(value))?;
                    if let Some(edge) = object.metadata() {
                        metadata.programs(edge, |owner| {
                            pending.push(Node::Program(programs.executable_edge(owner)?));
                            Some(())
                        })?;
                    }
                }
                Node::Program(id) => {
                    programs.trace(id, &mut |value| edges.push(value))?;
                    if !edges.iter().all(|value| self.validate_value(value)) {
                        return None;
                    }
                }
            }
            if self.resources.is_quarantined() {
                return None;
            }
            metadata.append_values(&mut edges);
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
        let graph = self.trace_graph(&programs, &values)?;
        for node in graph.nodes {
            match node {
                Node::Object(id) => {
                    live.insert(id);
                }
                Node::Program(id) => {
                    live_programs.insert(id);
                }
            }
        }
        // Evaluate payload accounting before mutating slots: it is a native hook.
        let dead = {
            let objects = self.objects.try_borrow().ok()?;
            objects
                .iter()
                .enumerate()
                .filter_map(|(index, slot)| {
                    let id = HeapObjectId::new(self.owner, index, slot.generation);
                    (!live.contains(&id))
                        .then(|| slot.object.as_ref().map(|object| (index, object.units())))
                        .flatten()
                })
                .collect::<Vec<_>>()
        };
        if self.resources.is_quarantined() {
            return None;
        }
        let dead_programs = programs.prepare_sweep(&live_programs);
        let mut groups = self.operation_groups.try_borrow_mut().ok()?;
        let mut applications = self.applications.try_borrow_mut().ok()?;
        let mut interfaces = self.interfaces.try_borrow_mut().ok()?;
        let mut environments = self.environments.try_borrow_mut().ok()?;
        let reclaimed_objects = dead.len();
        let reclaimed_units = dead.iter().map(|(_, units)| units).sum();
        let mut retired = Vec::with_capacity(reclaimed_objects);
        {
            let mut objects = self.objects.try_borrow_mut().ok()?;
            let mut free = self.free.try_borrow_mut().ok()?;
            for (index, _) in dead {
                let slot = &mut objects[index];
                retired.push(slot.object.take().expect("marked unreachable record"));
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    free.push(index);
                }
            }
        }
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
        self.release_heap_units(reclaimed_units);
        self.roots.borrow_mut().prune();
        // Destructors run with no storage-table borrow. Finish disposal even when
        // one panics; the execution gate keeps the affected runtime quarantined.
        for object in retired {
            if catch_unwind(AssertUnwindSafe(|| drop(object))).is_err() {
                self.resources
                    .quarantine("native payload destruction panicked");
            }
        }
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
        self.iterations.prune();
        self.iterator_loops.prune();
        self.mutations.prune();
        self.key_lookups.prune();
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
        Some(GcCollection {
            reclaimed_environments,
            reclaimed_operation_groups,
            reclaimed_method_applications,
            reclaimed_interface_snapshots,
            reclaimed_modules,
            reclaimed_objects,
            reclaimed_units,
            live_objects: live.len(),
            pause,
        })
    }
}
