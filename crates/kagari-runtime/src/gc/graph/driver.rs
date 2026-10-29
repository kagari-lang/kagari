//! Replacement owner-scanning driver. Root enumeration and sweep wiring join here.
use crate::{
    error::RuntimeError,
    execution_metadata::{MetadataEdge, scanning::MetadataCursor},
    gc::{
        GcHeap,
        directory::PayloadKind,
        graph::{
            GraphIdentity, GraphMarker, ScanKind,
            marks::MarkRef,
            objects::{ObjectCursor, ScanEdge},
        },
    },
    module::{
        ModuleStore,
        scanning::{ModuleCursor, StaticScanId},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
    value::Value,
};

#[derive(Debug)]
pub(crate) enum ActiveScan {
    Object(ObjectCursor),
    Struct { next_field: usize },
    Metadata(MetadataCursor),
    Module(ModuleCursor),
    Static(StaticScanId),
}

impl GcHeap {
    pub(crate) fn shade_graph_edge(
        &self,
        modules: &ModuleStore,
        edge: ScanEdge<'_>,
    ) -> Result<(), RuntimeError> {
        self.with_edge_mark(modules, edge, |id, mark, _| self.graph.shade(id, mark))?;
        Ok(())
    }

    pub(crate) fn shade_scanned_edge(
        &self,
        modules: &ModuleStore,
        edge: ScanEdge<'_>,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        Ok(self
            .with_edge_mark(modules, edge, |id, mark, kind| {
                self.graph.try_lock()?.shade_scanned(id, mark, kind, budget)
            })?
            .unwrap_or(ScanStatus::Complete))
    }

    pub(super) fn shade_scanned_edge_with(
        &self,
        modules: &ModuleStore,
        edge: ScanEdge<'_>,
        budget: &mut ScanBudget,
        graph: &mut GraphMarker<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        Ok(self
            .with_edge_mark(modules, edge, |id, mark, kind| {
                graph.shade_scanned(id, mark, kind, budget)
            })?
            .unwrap_or(ScanStatus::Complete))
    }

    fn with_edge_mark<R>(
        &self,
        modules: &ModuleStore,
        edge: ScanEdge<'_>,
        action: impl FnOnce(GraphIdentity, MarkRef<'_>, ScanKind) -> Result<R, RuntimeError>,
    ) -> Result<Option<R>, RuntimeError> {
        let reference = match edge {
            ScanEdge::Identity(GraphIdentity::Direct(reference))
            | ScanEdge::Value(Value::Struct(reference)) => Some(reference),
            _ => None,
        };
        if let Some(reference) = reference {
            let objects = self.objects.try_borrow().map_err(|_| invalid())?;
            let view = objects.resolve_direct(reference).ok_or_else(invalid)?;
            if matches!(edge, ScanEdge::Value(Value::Struct(_))) && view.fields.is_none() {
                return Err(invalid());
            }
            let kind = if view.edges {
                ScanKind::Edges
            } else {
                ScanKind::Leaf
            };
            return action(GraphIdentity::Direct(reference), view.mark, kind).map(Some);
        }
        let id = match edge {
            ScanEdge::Identity(id) => id,
            ScanEdge::Metadata(MetadataEdge::Program(owner)) => {
                return modules
                    .with_program_edge_mark(owner, |id, mark| action(id, mark, ScanKind::Edges))
                    .map(Some);
            }
            ScanEdge::Metadata(edge) => self.metadata_scan_identity(edge)?,
            ScanEdge::Value(value) => {
                if !value.is_managed() {
                    return Ok(None);
                }
                let objects = self.objects.try_borrow().map_err(|_| invalid())?;
                let identity = value.direct_reference().ok_or_else(invalid)?;
                let location = objects.direct_location(identity).ok_or_else(invalid)?;
                let taken =
                    matches!(value, Value::GcHandle(_)) && location.kind == PayloadKind::Taken;
                // Only collector tracing accepts an already retained tombstone.
                // Normal value admission still rejects it before any publication.
                if !taken && self.validate_heap_value(&objects, &value).is_none() {
                    return Err(invalid());
                }
                return action(
                    identity.graph(),
                    objects.mark(location).ok_or_else(invalid)?,
                    if taken {
                        ScanKind::Leaf
                    } else {
                        ScanKind::Edges
                    },
                )
                .map(Some);
            }
        };
        self.with_graph_mark(modules, id, |mark| action(id, mark, ScanKind::Edges))
            .map(Some)
    }

    /// One exclusive driver slice; no storage borrow or marker lock spans the
    /// yield. Faults preserve unfinished gray work and permanently forbid closure.
    pub(crate) fn scan_graph_slice(
        &self,
        modules: &ModuleStore,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        self.ensure_no_storage_borrow()?;
        let result = self
            .resources
            .collect_operation(|| self.scan_graph_inner(modules, budget))
            .and_then(|result| result);
        if result.is_err() {
            // Poisoned control state is already faulted by try_lock. Do not turn
            // that checked failure into an uncaught panic during fault cleanup.
            if let Ok(mut graph) = self.graph.try_lock() {
                graph.fault();
            }
            self.resources
                .quarantine("graph scanner failed before closure");
        }
        result
    }

    fn scan_graph_inner(
        &self,
        modules: &ModuleStore,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        // Keep the one authoritative control borrow across runtime-owned scans.
        // Callback/drop-capable branches below release it before entering the owner.
        let mut graph = self.graph.try_lock()?;
        loop {
            if graph.needs_owner() && !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let next =
                graph.next_owner(|group, epoch| self.next_group_owner(modules, group, epoch))?;
            let remaining = graph.needs_owner();
            let Some(id) = next else {
                if remaining {
                    continue;
                }
                return Ok(ScanStatus::Complete);
            };
            let mut active = self.graph_scan.try_borrow_mut().map_err(|_| invalid())?;
            if let GraphIdentity::Direct(reference) = id
                && !matches!(*active, Some(ActiveScan::Object(_)))
            {
                let objects = self.objects.try_borrow().map_err(|_| invalid())?;
                let view = objects.resolve_direct(reference).ok_or_else(invalid)?;
                if view.fields.is_some() {
                    let status = self.scan_struct_owner(
                        modules,
                        reference,
                        view,
                        &mut active,
                        budget,
                        &mut graph,
                    )?;
                    if status == ScanStatus::Pending {
                        return Ok(status);
                    }
                    continue;
                }
            }
            if active.is_none() {
                if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                *active = Some(match id {
                    GraphIdentity::Direct(reference) => {
                        ActiveScan::Object(self.begin_object_scan(reference)?)
                    }
                    GraphIdentity::Program(id) => {
                        ActiveScan::Module(modules.begin_module_scan(id)?)
                    }
                    GraphIdentity::Static(id) => ActiveScan::Static(id),
                    _ => ActiveScan::Metadata(self.begin_metadata_scan(id)?),
                });
            }
            let scan = active.as_mut().expect("started graph cursor");
            if let ActiveScan::Object(cursor) = scan
                && !self.object_scan_is_internal(cursor)?
            {
                // Native begin/resume invokes providers; HostPath advancement can
                // drop an Arc. Neither may inherit the collector control borrow.
                drop(graph);
                let status = self.scan_object_slice(cursor, budget, &mut |edge, budget| {
                    self.shade_scanned_edge(modules, edge, budget)
                })?;
                if status == ScanStatus::Pending {
                    return Ok(status);
                }
                graph = self.graph.try_lock()?;
            } else {
                let mut visit = |edge: ScanEdge<'_>, budget: &mut ScanBudget| {
                    self.shade_scanned_edge_with(modules, edge, budget, &mut graph)
                };
                let status = match scan {
                    ActiveScan::Object(cursor) => {
                        self.scan_object_slice(cursor, budget, &mut visit)?
                    }
                    ActiveScan::Metadata(cursor) => {
                        self.scan_metadata_slice(cursor, budget, &mut visit)?
                    }
                    ActiveScan::Module(cursor) => {
                        modules.scan_module_fields(cursor, budget, &mut visit)?
                    }
                    ActiveScan::Static(id) => modules.scan_static_slice(*id, budget, &mut visit)?,
                    ActiveScan::Struct { .. } => return Err(invalid()),
                };
                if status == ScanStatus::Pending {
                    return Ok(status);
                }
                if let ActiveScan::Module(cursor) = scan {
                    // Descriptor scan_finish may release an evicted entry and
                    // its dependencies. Keep that entire suffix on per-edge locks.
                    drop(graph);
                    let status =
                        modules.scan_module_descriptors(cursor, budget, &mut |edge, budget| {
                            self.shade_scanned_edge(modules, edge, budget)
                        })?;
                    if status == ScanStatus::Pending {
                        return Ok(status);
                    }
                    graph = self.graph.try_lock()?;
                }
            }
            self.with_graph_mark(modules, id, |mark| graph.finish_owner(id, mark))?;
            *active = None;
        }
    }

    /// Synchronous completion of the owner queue. It drains
    /// only already published work; it neither invents root completion nor sweeps.
    #[cfg(test)]
    pub(crate) fn drain_graph_work(&self, modules: &ModuleStore) -> Result<(), RuntimeError> {
        while self.scan_graph_slice(modules, &mut ScanBudget::new(usize::MAX))?
            == ScanStatus::Pending
        {}
        Ok(())
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid graph scan edge or cursor")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Runtime;

    #[test]
    fn borrowed_control_keeps_edge_validation_and_bitmap_precharge() {
        let runtime = Runtime::default();
        let foreign = Runtime::default();
        let value = runtime.gc.alloc_string("local".into()).unwrap();
        let other = foreign.gc.alloc_string("foreign".into()).unwrap();
        let id = value.direct_reference().unwrap();
        let mut graph = runtime.gc.graph.lock();
        graph.begin_cycle().unwrap();
        for invalid in [other, Value::Tuple(id)] {
            let mut budget = ScanBudget::new(4);
            assert!(
                runtime
                    .gc
                    .shade_scanned_edge_with(
                        runtime.modules(),
                        ScanEdge::Value(invalid),
                        &mut budget,
                        &mut graph
                    )
                    .is_err()
            );
            assert_eq!(budget.spent(), 0, "reject identity/kind before marking");
            assert!(graph.pending.is_empty());
        }
        let mut short = ScanBudget::new(3);
        assert_eq!(
            runtime
                .gc
                .shade_scanned_edge_with(
                    runtime.modules(),
                    ScanEdge::Value(value),
                    &mut short,
                    &mut graph
                )
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(short.spent(), 0);
        assert!(graph.pending.is_empty());
        let mut enough = ScanBudget::new(4);
        assert_eq!(
            runtime
                .gc
                .shade_scanned_edge_with(
                    runtime.modules(),
                    ScanEdge::Value(value),
                    &mut enough,
                    &mut graph
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert_eq!(enough.spent(), 4, "reset words remain prepaid");
        assert_eq!(graph.pending.len(), 1);
    }
}
