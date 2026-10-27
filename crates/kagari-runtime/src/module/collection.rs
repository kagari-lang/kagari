//! Synchronous retirement adapter over authoritative owner-local marks.
//! PG05 replaces the dead inventory with bounded owner cursors.
use crate::{
    gc::graph::GraphState,
    module::{
        ModuleKey, ModuleStore,
        records::ModuleRecord,
        state::cells::{CellId, StaticCell},
    },
};
use std::{sync::atomic::Ordering, vec::IntoIter};

pub(crate) struct ModuleSweep<'a> {
    modules: &'a ModuleStore,
    graph: &'a GraphState,
    restore_abandonment: bool,
}

/// Detached together with dead heap slots, then disposed outside storage borrows.
pub(crate) struct RetiredModule {
    pub(crate) key: ModuleKey,
    _record: Box<ModuleRecord>,
}

pub(crate) struct RetiredCell {
    _cell: StaticCell,
}

pub(crate) struct DeadPrograms {
    modules: IntoIter<ModuleKey>,
    cells: IntoIter<CellId>,
}

impl ModuleStore {
    pub(crate) fn begin_collection(&self) -> ModuleSweep<'_> {
        // A release racing with root enumeration must leave its own request set.
        ModuleSweep {
            modules: self,
            graph: &self.graph,
            restore_abandonment: self.abandoned.swap(false, Ordering::AcqRel),
        }
    }
}

impl Drop for ModuleSweep<'_> {
    fn drop(&mut self) {
        if self.restore_abandonment {
            self.modules.abandoned.store(true, Ordering::Release);
        }
    }
}

impl ModuleSweep<'_> {
    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn record_storage_usage(&self) {
        let store = self.modules.inner.borrow();
        self.graph
            .lock()
            .record_module_storage(store.records.storage_usage());
    }

    pub(crate) fn prepare_sweep(&self) -> Option<DeadPrograms> {
        let store = self.modules.inner.try_borrow_mut().ok()?;
        let graph = self.graph.try_lock().ok()?;
        let mut dead = Vec::new();
        for (key, record) in store.records.iter() {
            let id = store.records.scan_id(&record.module)?;
            if !graph.is_survivor(store.records.mark(id)?).ok()? {
                dead.push(*key);
            }
        }
        let mut cells = Vec::new();
        for (id, cell) in &store.cells {
            if !graph.is_survivor(cell.mark.as_ref()).ok()? {
                graph.can_retire(cell.mark.as_ref()).ok()?;
                cells.push(*id);
            }
        }
        Some(DeadPrograms {
            modules: dead.into_iter(),
            cells: cells.into_iter(),
        })
    }

    /// Call only after the entire graph and heap accounting have been validated.
    pub(crate) fn detach_program_batch(
        &mut self,
        dead: &mut DeadPrograms,
        limit: usize,
    ) -> Vec<RetiredModule> {
        assert!(limit != 0);
        let mut store = self.modules.inner.borrow_mut();
        let mut graph = self.graph.lock();
        dead.modules
            .by_ref()
            .take(limit)
            .map(|key| {
                store.retentions.remove(&key);
                store.staged.remove(&key);
                RetiredModule {
                    key,
                    _record: store
                        .records
                        .remove(&key, &mut graph)
                        .expect("dead module record"),
                }
            })
            .collect()
    }

    pub(crate) fn detach_cell_batch(
        &mut self,
        dead: &mut DeadPrograms,
        limit: usize,
    ) -> Vec<RetiredCell> {
        assert!(limit != 0);
        let mut store = self.modules.inner.borrow_mut();
        let mut graph = self.graph.lock();
        dead.cells
            .by_ref()
            .take(limit)
            .map(|id| {
                let cell = store.cells.remove(&id).expect("dead static cell");
                graph.release_singleton(cell.mark.as_ref());
                graph.retire_prepared();
                RetiredCell { _cell: cell }
            })
            .collect()
    }

    pub(crate) fn finish(&mut self, dead: DeadPrograms) {
        assert!(dead.modules.len() == 0 && dead.cells.len() == 0);
        self.restore_abandonment = false;
    }
}
