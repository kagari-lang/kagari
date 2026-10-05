//! Runtime-owned session records and independently borrowed frame stacks.
use crate::{
    error::RuntimeError,
    frame::ExecutionFrame,
    module::{LoadedModule, retention::ProgramLease},
    resource::ResourceCounters,
    session::{ExecutionOptions, SessionState},
};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    collections::HashMap,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SessionId {
    owner: u64,
    slot: usize,
    generation: u64,
}

#[derive(Debug)]
struct SessionSlot {
    generation: u64,
    state: Option<SessionState>,
}

#[derive(Debug, Default)]
struct Records {
    slots: Vec<SessionSlot>,
    free: Vec<usize>,
}

#[derive(Debug)]
pub(crate) struct SessionStore {
    owner: u64,
    records: RefCell<Records>,
    frames: RefCell<HashMap<SessionId, Vec<ExecutionFrame>>>,
    retired_frames: Cell<bool>,
}

impl Default for SessionStore {
    fn default() -> Self {
        static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);
        let owner = NEXT_OWNER
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("session store identity exhausted");
        Self {
            owner,
            records: RefCell::new(Records::default()),
            frames: RefCell::new(HashMap::new()),
            retired_frames: Cell::new(false),
        }
    }
}

impl SessionStore {
    fn prune_frames(
        &self,
        frames: &mut HashMap<SessionId, Vec<ExecutionFrame>>,
        records: &Records,
    ) {
        if self.retired_frames.replace(false) {
            frames.retain(|id, _| {
                records
                    .slots
                    .get(id.slot)
                    .is_some_and(|slot| slot.generation == id.generation && slot.state.is_some())
            });
        }
    }

    pub(crate) fn insert(
        &self,
        root: LoadedModule,
        options: ExecutionOptions,
        baseline: ResourceCounters,
        program: ProgramLease,
    ) -> Result<SessionId, RuntimeError> {
        let mut records = self.records.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("session records borrowed across session entry")
        })?;
        let mut frames = self.frames.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("frame stack borrowed across session entry")
        })?;
        self.prune_frames(&mut frames, &records);
        frames
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("session frames"))?;
        if records.free.is_empty() {
            records
                .slots
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("session slots"))?;
        }
        let index = records.free.pop().unwrap_or_else(|| {
            records.slots.push(SessionSlot {
                generation: 0,
                state: None,
            });
            records.slots.len() - 1
        });
        let slot = &mut records.slots[index];
        let id = SessionId {
            owner: self.owner,
            slot: index,
            generation: slot.generation,
        };
        slot.state = Some(SessionState::new(id, root, options, baseline, program));
        frames.insert(id, Vec::new());
        Ok(id)
    }

    pub(crate) fn get(&self, id: SessionId) -> Option<Ref<'_, SessionState>> {
        if id.owner != self.owner {
            return None;
        }
        Ref::filter_map(self.records.borrow(), |records| {
            let slot = records.slots.get(id.slot)?;
            (slot.generation == id.generation)
                .then_some(slot.state.as_ref())
                .flatten()
        })
        .ok()
    }

    pub(crate) fn frames(&self, id: SessionId) -> Option<Ref<'_, Vec<ExecutionFrame>>> {
        self.get(id)?;
        Ref::filter_map(self.frames.try_borrow().ok()?, |frames| frames.get(&id)).ok()
    }

    pub(crate) fn frames_mut(&self, id: SessionId) -> Option<RefMut<'_, Vec<ExecutionFrame>>> {
        self.get(id)?;
        let mut frames = self.frames.try_borrow_mut().ok()?;
        if self.retired_frames.get() {
            self.prune_frames(&mut frames, &self.records.borrow());
        }
        RefMut::filter_map(frames, |frames| frames.get_mut(&id)).ok()
    }

    /// Detach records before dropping them; their members may release other resources.
    pub(crate) fn remove(&self, id: SessionId) -> Option<(SessionState, Vec<ExecutionFrame>)> {
        if id.owner != self.owner {
            return None;
        }
        let mut records = self.records.try_borrow_mut().ok()?;
        let mut frames = self.frames.try_borrow_mut().ok();
        // An immutable view of a different session must not postpone semantic
        // cleanup. Only an empty bucket may wait for the next mutable access;
        // roots, program leases and session state are detached immediately.
        if frames.is_none() && !self.frames.try_borrow().ok()?.get(&id)?.is_empty() {
            return None;
        }
        let slot = records.slots.get_mut(id.slot)?;
        if slot.generation != id.generation {
            return None;
        }
        let state = slot.state.take()?;
        let frames = match frames.as_mut() {
            Some(frames) => frames.remove(&id).expect("session owns its frame stack"),
            None => {
                self.retired_frames.set(true);
                Vec::new()
            }
        };
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            records.free.push(id.slot);
        }
        Some((state, frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Runtime, module::ModuleEpochRetention};
    use kagari_bytecode::{
        module::BytecodeModule,
        program::{BytecodeProgram, ModuleRef},
    };

    fn module() -> (Runtime, LoadedModule) {
        let mut runtime = Runtime::default();
        let module = runtime
            .load_program(
                "sessions",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap();
        (runtime, module)
    }

    fn insert(store: &SessionStore, runtime: &Runtime, module: &LoadedModule) -> SessionId {
        let lease = runtime
            .retain_module(module, ModuleEpochRetention::ActiveCall)
            .unwrap();
        store
            .insert(
                module.clone(),
                ExecutionOptions::default(),
                ResourceCounters::default(),
                lease,
            )
            .unwrap()
    }

    #[test]
    fn retired_session_ids_cannot_read_or_remove_reused_records_and_frames() {
        let (runtime, module) = module();
        let store = SessionStore::default();
        let foreign = SessionStore::default();
        let first = insert(&store, &runtime, &module);
        let foreign_id = insert(&foreign, &runtime, &module);
        assert!(store.get(foreign_id).is_none());
        assert!(store.frames(foreign_id).is_none());
        assert!(store.frames_mut(foreign_id).is_none());
        assert!(store.remove(foreign_id).is_none());
        let inspected = store.frames(first).unwrap();
        drop(store.remove(first).unwrap());
        assert!(store.frames(first).is_none());
        assert!(inspected.is_empty());
        drop(inspected);
        let second = insert(&store, &runtime, &module);
        assert_eq!(first.slot, second.slot);
        assert_ne!(first.generation, second.generation);
        assert!(store.get(first).is_none());
        assert!(store.frames(first).is_none());
        assert!(store.frames_mut(first).is_none());
        assert!(store.remove(first).is_none());
        assert_eq!(store.get(second).unwrap().root.key(), module.key());
        assert!(store.frames(second).unwrap().is_empty());
        assert_eq!(store.frames.borrow().len(), 1);
    }

    #[test]
    fn exhausted_generations_retire_session_slots() {
        let store = SessionStore::default();
        store.records.borrow_mut().slots.push(SessionSlot {
            generation: u64::MAX,
            state: None,
        });
        store.records.borrow_mut().free.push(0);
        let (runtime, module) = module();
        let last = insert(&store, &runtime, &module);
        assert_eq!(last.slot, 0);
        assert_eq!(last.generation, u64::MAX);
        drop(store.remove(last).unwrap());
        let next = insert(&store, &runtime, &module);
        assert_eq!(next.slot, 1);
        assert!(store.get(last).is_none());
        assert!(store.frames(last).is_none());
    }

    #[test]
    fn frame_borrows_allow_session_checks_but_reject_conflicting_store_edits() {
        let (runtime, module) = module();
        let store = SessionStore::default();
        let id = insert(&store, &runtime, &module);
        let frames = store.frames_mut(id).unwrap();
        assert_eq!(store.get(id).unwrap().root.key(), module.key());
        assert!(
            store
                .insert(
                    module.clone(),
                    ExecutionOptions::default(),
                    ResourceCounters::default(),
                    runtime
                        .retain_module(&module, ModuleEpochRetention::ActiveCall)
                        .unwrap()
                )
                .is_err()
        );
        assert!(store.remove(id).is_none());
        drop(frames);
        assert_eq!(store.records.borrow().slots.len(), 1);
        assert_eq!(store.frames.borrow().len(), 1);
        drop(store.remove(id).unwrap());
        assert!(store.get(id).is_none());
        assert!(store.frames(id).is_none());
    }
}
