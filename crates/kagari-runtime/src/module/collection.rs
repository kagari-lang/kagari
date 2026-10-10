//! Module instances participate in the same graph as heap objects.
use crate::{
    execution_metadata::MetadataEdge,
    module::{
        LoadedModule, ModuleKey, ModuleStore, ModuleStoreInner, records::ModuleRecord,
        root_programs,
    },
    value::Value,
};
use std::{
    cell::RefMut,
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

/// Exclusive access prevents instance writes between marking and detachment.
pub(crate) struct ProgramGraph<'a> {
    store: RefMut<'a, ModuleStoreInner>,
    abandoned: &'a AtomicBool,
    restore_abandonment: bool,
}

/// Detached together with dead heap slots, then disposed outside storage borrows.
pub(crate) struct RetiredModule {
    pub(crate) key: ModuleKey,
    _record: ModuleRecord,
}

pub(crate) struct DeadPrograms(Vec<ModuleKey>);

impl ModuleStore {
    pub(crate) fn collection_graph(&self) -> Option<ProgramGraph<'_>> {
        let store = self.inner.try_borrow_mut().ok()?;
        // Acknowledge before tracing. Releases racing with this collection leave
        // their own request set, including programs already reached by marking.
        Some(ProgramGraph {
            store,
            abandoned: &self.abandoned,
            restore_abandonment: self.abandoned.swap(false, Ordering::AcqRel),
        })
    }
}

impl Drop for ProgramGraph<'_> {
    fn drop(&mut self) {
        if self.restore_abandonment {
            self.abandoned.store(true, Ordering::Release);
        }
    }
}

impl ProgramGraph<'_> {
    pub(crate) fn roots(&self) -> HashSet<ModuleKey> {
        root_programs(&self.store)
    }

    pub(crate) fn executable_edge(&self, module: &LoadedModule) -> Option<ModuleKey> {
        Some(self.store.resolve(module)?.module.program_key())
    }

    pub(crate) fn trace<'a>(
        &'a self,
        key: ModuleKey,
        visit: &mut dyn FnMut(&'a Value),
        metadata: &mut Vec<MetadataEdge<'a>>,
    ) -> Option<()> {
        let program = &self.store.records.get(&key)?.module;
        if program.program_key() != key {
            return None;
        }
        for member in program.members() {
            let record = self.store.records.get(&member.key())?;
            record.descriptors.trace(&self.store, metadata);
            if let Some(calls) = &record.calls {
                calls.trace(metadata);
            }
            record
                .constants
                .iter()
                .flatten()
                .rev()
                .for_each(&mut *visit);
            record
                .instance
                .module_slots
                .iter()
                .rev()
                .for_each(&mut *visit);
        }
        Some(())
    }

    pub(crate) fn prepare_sweep(&self, live: &HashSet<ModuleKey>) -> DeadPrograms {
        let dead = self
            .store
            .records
            .iter()
            .filter_map(|(key, module)| {
                (!live.contains(&module.module.program_key())).then_some(*key)
            })
            .collect::<Vec<_>>();
        DeadPrograms(dead)
    }

    /// Call only after the entire graph and heap accounting have been validated.
    pub(crate) fn detach(&mut self, dead: DeadPrograms) -> Vec<RetiredModule> {
        let retired = dead
            .0
            .into_iter()
            .map(|key| {
                self.store.retentions.remove(&key);
                self.store.staged.remove(&key);
                RetiredModule {
                    key,
                    _record: self.store.records.remove(&key).expect("dead module record"),
                }
            })
            .collect::<Vec<_>>();
        for retained in self.store.retentions.values_mut() {
            retained.prune();
        }
        self.restore_abandonment = false;
        retired
    }
}
