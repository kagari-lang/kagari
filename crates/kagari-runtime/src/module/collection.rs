//! Owned collection requests and bounded sweeps over the original module/cell stores.
use crate::{
    error::RuntimeError,
    execution_metadata::scanning::RetirementBatch,
    gc::graph::MarkPhase,
    module::{
        ModuleKey, ModuleStore,
        records::ModuleRecord,
        state::cells::{StaticCell, StaticSweep},
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Consumes only the request observed at cycle entry. Aborting restores that
/// request; successful completion never clears a newer concurrent release.
pub(crate) struct AbandonmentTicket {
    abandoned: Arc<AtomicBool>,
    restore: bool,
}

impl AbandonmentTicket {
    pub(crate) fn finish(mut self) {
        self.restore = false;
    }
}

impl Drop for AbandonmentTicket {
    fn drop(&mut self) {
        if self.restore {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}

/// No borrowed record or dead inventory survives a slice. The cell continuation
/// is repaired inside its original owner whenever an occupied node is removed.
pub(crate) struct ModuleSweep {
    owner: Arc<AtomicBool>,
    next: usize,
    end: usize,
    cells: StaticSweep,
}

/// Destroy only after the sweep method releases its module-store and graph borrows.
pub(crate) struct RetiredModule {
    _record: Box<ModuleRecord>,
}

pub(crate) struct RetiredCell {
    _cell: StaticCell,
}

impl ModuleStore {
    pub(crate) fn begin_collection(&self) -> AbandonmentTicket {
        AbandonmentTicket {
            abandoned: self.abandoned.clone(),
            restore: self.abandoned.swap(false, Ordering::AcqRel),
        }
    }

    /// Preflight the shared module/static owner without retaining a borrow across
    /// another owner's destruction. Called under the collector's execution exclusion.
    pub(crate) fn check_sweep_access(&self) -> Result<(), RuntimeError> {
        let _store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
        Ok(())
    }

    pub(crate) fn begin_sweep(&self) -> Result<ModuleSweep, RuntimeError> {
        let mut store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
        let graph = self.graph.try_lock()?;
        if graph.phase() != MarkPhase::Sweep {
            return Err(invalid());
        }
        Ok(ModuleSweep {
            owner: self.abandoned.clone(),
            next: 0,
            end: store.records.scan_extent(),
            cells: store.cells.begin_sweep()?,
        })
    }

    /// Charge every stable slot, including vacancies and newborn/surviving records.
    /// Report and disposal backing grow only at the first confirmed dead record,
    /// before any removal, with capacity for the remaining bounded batch.
    /// `limit` is the driver's remaining allowance out of 32 total retired owners.
    /// Append into caller-owned backing so partial errors leave every detached
    /// record available to the driver's panic-contained disposal path.
    pub(crate) fn sweep_programs(
        &self,
        cursor: &mut ModuleSweep,
        budget: &mut ScanBudget,
        limit: usize,
        mut report: Option<&mut Vec<ModuleKey>>,
        retired: &mut Vec<RetiredModule>,
    ) -> Result<ScanStatus, RuntimeError> {
        let batch = RetirementBatch::new(retired.len(), limit)?;
        if !Arc::ptr_eq(&cursor.owner, &self.abandoned) {
            return Err(invalid());
        }
        let mut store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
        let mut graph = self.graph.try_lock()?;
        if graph.phase() != MarkPhase::Sweep {
            return Err(invalid());
        }
        store.cells.sweep_next(&cursor.cells)?;
        let remaining = cursor.end.checked_sub(cursor.next).ok_or_else(invalid)?;
        if remaining == 0 {
            return Ok(ScanStatus::Complete);
        }
        while cursor.next < cursor.end && !batch.is_full(retired.len()) {
            if !budget.take(1) {
                break;
            }
            let index = cursor.next;
            let Some((id, record)) = store.records.scan_slot(index) else {
                cursor.next += 1;
                continue;
            };
            let mark = store.records.mark(id).ok_or_else(invalid)?;
            if graph.is_survivor(mark)? {
                cursor.next += 1;
                continue;
            }
            graph.can_retire(mark)?;
            let additional = batch.prepare(retired, cursor.end - cursor.next)?;
            if additional != 0
                && let Some(report) = &mut report
            {
                report
                    .try_reserve(additional)
                    .map_err(|_| RuntimeError::resource_limit("reclaimed module report"))?;
            }
            let key = record.module.key();
            let record = store.records.remove(&key, &mut graph).ok_or_else(invalid)?;
            store.retentions.remove(&key);
            store.staged.remove(&key);
            if let Some(report) = &mut report {
                report.push(key);
            }
            retired.push(RetiredModule { _record: record });
            cursor.next += 1;
        }
        let status = if cursor.next == cursor.end {
            ScanStatus::Complete
        } else {
            ScanStatus::Pending
        };
        Ok(status)
    }

    /// Each occupied cell costs one inspection. Prepending a new cell does not
    /// extend the captured chain; removals repair its next node in constant work.
    /// As with modules, detached cells stay in the caller's buffer on any error.
    pub(crate) fn sweep_cells(
        &self,
        cursor: &mut ModuleSweep,
        budget: &mut ScanBudget,
        limit: usize,
        retired: &mut Vec<RetiredCell>,
    ) -> Result<ScanStatus, RuntimeError> {
        let batch = RetirementBatch::new(retired.len(), limit)?;
        if !Arc::ptr_eq(&cursor.owner, &self.abandoned) {
            return Err(invalid());
        }
        let mut store = self.inner.try_borrow_mut().map_err(|_| invalid())?;
        let mut graph = self.graph.try_lock()?;
        if graph.phase() != MarkPhase::Sweep {
            return Err(invalid());
        }
        let Some(mut id) = store.cells.sweep_next(&cursor.cells)? else {
            return Ok(ScanStatus::Complete);
        };
        loop {
            if batch.is_full(retired.len()) || !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let cell = store.cells.get(&id).ok_or_else(invalid)?;
            if graph.is_survivor(cell.mark.as_ref())? {
                store.cells.advance_sweep(&cursor.cells, id)?;
            } else {
                graph.can_retire(cell.mark.as_ref())?;
                // Occupied count bounds the captured suffix without traversing it.
                // Newborn prepends cannot extend the continuation.
                batch.prepare(retired, store.cells.len())?;
                let cell = store.cells.remove(&id, &mut graph)?.ok_or_else(invalid)?;
                retired.push(RetiredCell { _cell: cell });
            }
            let Some(next) = store.cells.sweep_next(&cursor.cells)? else {
                return Ok(ScanStatus::Complete);
            };
            id = next;
        }
    }

    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn record_collection_storage_usage(&self) {
        let store = self.inner.borrow();
        self.graph
            .lock()
            .record_module_storage(store.records.storage_usage());
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("invalid or borrowed module sweep")
}

#[cfg(test)]
mod tests;
