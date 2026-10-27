//! Replacement owner-scanning driver. Root enumeration and sweep wiring join here.
use crate::{
    error::RuntimeError,
    execution_metadata::{MetadataEdge, scanning::MetadataCursor},
    gc::{
        GcHeap,
        graph::{
            GraphIdentity,
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
    Metadata(MetadataCursor),
    Module(ModuleCursor),
    Static(StaticScanId),
    Moved,
}

impl GcHeap {
    pub(crate) fn shade_graph_edge(
        &self,
        modules: &ModuleStore,
        edge: ScanEdge<'_>,
    ) -> Result<(), RuntimeError> {
        let id = match edge {
            ScanEdge::Identity(id) => id,
            ScanEdge::Metadata(MetadataEdge::Program(owner)) => {
                return modules.shade_program_edge(owner);
            }
            ScanEdge::Metadata(edge) => self.metadata_scan_identity(edge)?,
            ScanEdge::Value(value) => {
                let Some(id) = value.object_id() else {
                    return Ok(());
                };
                if matches!(value, Value::GcHandle(_))
                    && self
                        .moved
                        .try_borrow()
                        .map_err(|_| invalid())?
                        .contains_key(&id)
                {
                    GraphIdentity::Moved(id)
                } else {
                    return self.publish_value(value);
                }
            }
        };
        self.shade_graph_identity(modules, id)
    }

    /// One exclusive driver slice; no storage borrow or marker lock spans the
    /// yield. Faults preserve unfinished gray work and permanently forbid closure.
    pub(crate) fn scan_graph_slice(
        &self,
        modules: &ModuleStore,
        budget: &mut ScanBudget,
    ) -> Result<ScanStatus, RuntimeError> {
        self.ensure_no_native_borrow()?;
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
        loop {
            let mut graph = self.graph.try_lock()?;
            if !graph.charge_bitmap_work(budget) {
                return Ok(ScanStatus::Pending);
            }
            if graph.needs_owner() && !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let next =
                graph.next_owner(|group, epoch| self.next_group_owner(modules, group, epoch))?;
            let remaining = graph.needs_owner();
            drop(graph);
            let Some(mut id) = next else {
                if remaining {
                    continue;
                }
                return Ok(ScanStatus::Complete);
            };
            let mut active = self.graph_scan.try_borrow_mut().map_err(|_| invalid())?;
            if let GraphIdentity::Object(object) = id
                && self
                    .moved
                    .try_borrow()
                    .map_err(|_| invalid())?
                    .contains_key(&object)
            {
                if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                let moved = GraphIdentity::Moved(object);
                self.graph.try_lock()?.retarget_taken_owner(id, moved)?;
                if active.is_some() {
                    self.native_scan_workspace
                        .try_borrow_mut()
                        .map_err(|_| invalid())?
                        .abandon_taken()?;
                }
                *active = Some(ActiveScan::Moved);
                id = moved;
            }
            if active.is_none() {
                if !budget.take(1) {
                    return Ok(ScanStatus::Pending);
                }
                *active = Some(match id {
                    GraphIdentity::Object(id) => ActiveScan::Object(self.begin_object_scan(id)?),
                    GraphIdentity::Program(id) => {
                        ActiveScan::Module(modules.begin_module_scan(id)?)
                    }
                    GraphIdentity::Static(id) => ActiveScan::Static(id),
                    GraphIdentity::Moved(_) => ActiveScan::Moved,
                    _ => ActiveScan::Metadata(self.begin_metadata_scan(id)?),
                });
            }
            let mut visit = |edge: ScanEdge<'_>| self.shade_graph_edge(modules, edge);
            let status = match active.as_mut().expect("started graph cursor") {
                ActiveScan::Object(cursor) => self.scan_object_slice(cursor, budget, &mut visit)?,
                ActiveScan::Metadata(cursor) => {
                    self.scan_metadata_slice(cursor, budget, &mut visit)?
                }
                ActiveScan::Module(cursor) => {
                    modules.scan_module_slice(cursor, budget, &mut visit)?
                }
                ActiveScan::Static(id) => modules.scan_static_slice(*id, budget, &mut visit)?,
                ActiveScan::Moved => ScanStatus::Complete,
            };
            if status == ScanStatus::Pending {
                return Ok(status);
            }
            self.with_graph_mark(modules, id, |mark| {
                self.graph.try_lock()?.finish_owner(id, mark)
            })?;
            *active = None;
        }
    }

    /// Synchronous completion of the owner queue. It drains
    /// only already published work; it neither invents root completion nor sweeps.
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
