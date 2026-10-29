//! A struct's immutable reference map and mark share one uninterrupted owner borrow.
#[cfg(test)]
mod tests;

use crate::{
    error::RuntimeError,
    gc::{
        GcHeap,
        directory::ResolvedDirect,
        graph::{GraphIdentity, GraphMarker, driver::ActiveScan, objects::ScanEdge},
        reference::GcRef,
    },
    module::ModuleStore,
    native::storage::scan::{ScanBudget, ScanStatus},
};

impl GcHeap {
    /// Resolve once at begin/resume, then use the same fields and mark through
    /// completion. Only numeric progress survives Pending. There are no provider
    /// calls, mutation, allocation or drop-capable continuations in this slice.
    pub(super) fn scan_struct_owner(
        &self,
        modules: &ModuleStore,
        reference: GcRef,
        view: ResolvedDirect<'_>,
        active: &mut Option<ActiveScan>,
        budget: &mut ScanBudget,
        graph: &mut GraphMarker<'_>,
    ) -> Result<ScanStatus, RuntimeError> {
        if active.is_none() {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            *active = Some(ActiveScan::Struct { next_field: 0 });
        }
        let Some(ActiveScan::Struct { next_field }) = active else {
            return Err(invalid());
        };
        let fields = view.fields().ok_or_else(invalid)?;
        while let Some(slot) = fields.description.references.get(*next_field) {
            if !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let value = fields.get(*slot).ok_or_else(invalid)?;
            if self.shade_scanned_edge_with(modules, ScanEdge::Value(value), budget, graph)?
                == ScanStatus::Pending
            {
                return Ok(ScanStatus::Pending);
            }
            *next_field += 1;
        }
        graph.finish_owner(GraphIdentity::Direct(reference), view.mark)?;
        *active = None;
        Ok(ScanStatus::Complete)
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed struct scan owner")
}
