//! Runtime-owned session records and independently borrowed frame stacks.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::BufferUsage;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    error::RuntimeError,
    execution::roots::{DependencyId, SessionId},
    frame::ExecutionFrame,
    module::{
        ExecutableModule, managed::revision::RevisionState, retention::RetentionRegistration,
    },
    resource::ResourceCounters,
    session::{ExecutionOptions, SessionState},
};
use std::{
    cell::{Ref, RefCell, RefMut},
    mem,
};

#[derive(Debug, Default)]
struct Records {
    slots: Vec<Option<SessionState>>,
}

/// Control records keyed by identities minted and reused by ExecutionRoots.
/// Every lookup compares the complete source identity, not just its coordinate.
#[derive(Debug, Default)]
pub(crate) struct SessionStore {
    records: RefCell<Records>,
    frames: RefCell<Vec<Vec<ExecutionFrame>>>,
}

impl SessionStore {
    #[cfg(feature = "execution-diagnostics")]
    pub(crate) fn visit_usage(&self, visit: &mut impl FnMut(BufferUsage)) {
        let records = self.records.borrow();
        let frames = self.frames.borrow();
        for buffer in [
            BufferUsage::new("session records", &records.slots),
            BufferUsage::new("frame directories", &frames),
        ] {
            visit(buffer);
        }
        for frames in frames.iter() {
            visit(BufferUsage::new("execution frames", frames));
        }
        for state in records.slots.iter().flatten() {
            visit(BufferUsage::new(
                "frame scopes",
                &state.frame_scopes.borrow(),
            ));
            visit(BufferUsage::new(
                "host call traces",
                &state.host_calls.borrow(),
            ));
        }
    }

    /// Inspect control stacks in one closed metadata-only interval. The caller
    /// copies its needed facts before borrowing Runtime or source services.
    pub(crate) fn inspect_frame_stacks<R>(
        &self,
        inspect: impl FnOnce(&[Vec<ExecutionFrame>]) -> R,
    ) -> Option<R> {
        let frames = self.frames.try_borrow().ok()?;
        Some(inspect(&frames))
    }

    pub(crate) fn detach_observers(&self) {
        for state in self.records.borrow().slots.iter().flatten() {
            state.observer_attached.set(false);
        }
    }

    pub(crate) fn retired_owners(&self, quarantined: bool) -> Vec<SessionId> {
        self.records
            .borrow()
            .slots
            .iter()
            .filter_map(|slot| {
                let state = slot.as_ref()?;
                let owner = state.owner.borrow();
                (state.scopes.get() == 0
                    && owner
                        .as_ref()
                        .is_some_and(|owner| quarantined || owner.abandoned()))
                .then_some(state.id)
            })
            .collect()
    }

    pub(crate) fn insert(
        &self,
        id: SessionId,
        root: ExecutableModule,
        options: ExecutionOptions,
        baseline: ResourceCounters,
        registration: RetentionRegistration,
        body: DependencyId<RevisionState>,
    ) -> Result<SessionId, RuntimeError> {
        if body.execution() != id {
            return Err(RuntimeError::module_validation(
                "session body belongs to another execution",
            ));
        }
        let mut records = self.records.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("session records borrowed across session entry")
        })?;
        let mut frames = self.frames.try_borrow_mut().map_err(|_| {
            RuntimeError::module_validation("frame stack borrowed across session entry")
        })?;
        let index = id.index();
        if records.slots.get(index).is_some_and(Option::is_some) {
            return Err(RuntimeError::module_validation(
                "execution control slot is occupied",
            ));
        }
        let count = index
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("session slots"))?;
        let record_len = records.slots.len();
        let frame_len = frames.len();
        records
            .slots
            .try_reserve(count.saturating_sub(record_len))
            .map_err(|_| RuntimeError::resource_limit("session slots"))?;
        frames
            .try_reserve(count.saturating_sub(frame_len))
            .map_err(|_| RuntimeError::resource_limit("session frames"))?;
        records.slots.resize_with(count.max(record_len), || None);
        frames.resize_with(count.max(frame_len), Vec::new);
        debug_assert!(frames[index].is_empty());
        records.slots[index] = Some(SessionState::new(
            id,
            root,
            options,
            baseline,
            registration,
            body,
        ));
        Ok(id)
    }

    pub(crate) fn get(&self, id: SessionId) -> Option<Ref<'_, SessionState>> {
        Ref::filter_map(self.records.borrow(), |records| {
            records
                .slots
                .get(id.index())?
                .as_ref()
                .filter(|state| state.id == id)
        })
        .ok()
    }

    pub(crate) fn frames(&self, id: SessionId) -> Option<Ref<'_, Vec<ExecutionFrame>>> {
        self.get(id)?;
        Ref::filter_map(self.frames.try_borrow().ok()?, |frames| {
            frames.get(id.index())
        })
        .ok()
    }

    pub(crate) fn frames_mut(&self, id: SessionId) -> Option<RefMut<'_, Vec<ExecutionFrame>>> {
        self.get(id)?;
        let frames = self.frames.try_borrow_mut().ok()?;
        RefMut::filter_map(frames, |frames| frames.get_mut(id.index())).ok()
    }

    /// Detach records before dropping them; their members may release other resources.
    pub(crate) fn remove(&self, id: SessionId) -> Option<(SessionState, Vec<ExecutionFrame>)> {
        let mut records = self.records.try_borrow_mut().ok()?;
        let mut frames = self.frames.try_borrow_mut().ok();
        // An immutable view of a different session must not postpone semantic
        // cleanup. Only an empty bucket may wait for the next mutable access;
        // control registrations and session state are detached immediately; its owner
        // separately retires the complete execution source.
        if frames.is_none() && !self.frames.try_borrow().ok()?.get(id.index())?.is_empty() {
            return None;
        }
        let slot = records.slots.get_mut(id.index())?;
        if slot.as_ref()?.id != id {
            return None;
        }
        let state = slot.take()?;
        let frames = match frames.as_mut() {
            Some(frames) if !frames[id.index()].is_empty() => mem::take(&mut frames[id.index()]),
            Some(_) | None => Vec::new(),
        };
        Some((state, frames))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::native::program::ProgramFixture;
    use crate::{
        Runtime, execution::roots::ExecutionRoots, module::managed::source::RevisionSource,
    };
    use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
    use kagari_common::identity::map::DefinitionContext;

    fn fixture() -> (Runtime, ExecutableModule) {
        let context = DefinitionContext::new().unwrap();
        let mut runtime = Runtime::default();
        let program = load_bytecode(
            &mut runtime,
            "sessions",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
        let module = runtime.select_program(&program).unwrap();
        (runtime, module)
    }

    fn body(
        runtime: &mut Runtime,
        module: &ExecutableModule,
        id: SessionId,
    ) -> DependencyId<RevisionState> {
        runtime
            .memory
            .scope(&runtime.resources, |cx, roots| {
                ExecutionRoots::with(roots, cx, |slots| {
                    let revision =
                        RevisionSource::Current.resolve(&runtime.modules, slots, module)?;
                    slots.push_dependency(id, revision)
                })
            })
            .unwrap()
    }

    fn insert(store: &SessionStore, runtime: &mut Runtime, module: &ExecutableModule) -> SessionId {
        let id = runtime.memory.enter_execution(&runtime.resources).unwrap();
        let body = body(runtime, module, id);
        store
            .insert(
                id,
                module.clone(),
                ExecutionOptions::default(),
                ResourceCounters::default(),
                runtime.modules.register_execution(module).unwrap(),
                body,
            )
            .unwrap()
    }

    #[test]
    fn retired_session_ids_cannot_read_or_remove_reused_records_and_frames() {
        let (mut runtime, module) = fixture();
        let store = SessionStore::default();
        let foreign = SessionStore::default();
        let (mut foreign_runtime, foreign_module) = fixture();
        let first = insert(&store, &mut runtime, &module);
        let foreign_id = insert(&foreign, &mut foreign_runtime, &foreign_module);
        assert!(store.get(foreign_id).is_none());
        assert!(store.frames(foreign_id).is_none());
        assert!(store.frames_mut(foreign_id).is_none());
        assert!(store.remove(foreign_id).is_none());
        let inspected = store.frames(first).unwrap();
        drop(store.remove(first).unwrap());
        runtime
            .memory
            .retire_execution(&runtime.resources, first)
            .unwrap();
        assert!(store.frames(first).is_none());
        assert!(inspected.is_empty());
        drop(inspected);
        let second = insert(&store, &mut runtime, &module);
        assert_eq!(first.index(), second.index());
        assert_ne!(first, second);
        assert!(store.get(first).is_none());
        assert!(store.frames(first).is_none());
        assert!(store.frames_mut(first).is_none());
        assert!(store.remove(first).is_none());
        assert_eq!(store.get(second).unwrap().root.key(), module.key());
        assert!(store.frames(second).unwrap().is_empty());
        assert_eq!(store.frames.borrow().len(), 1);
    }

    #[test]
    fn frame_borrows_allow_session_checks_but_reject_conflicting_store_edits() {
        let (mut runtime, module) = fixture();
        let store = SessionStore::default();
        let id = insert(&store, &mut runtime, &module);
        let frames = store.frames_mut(id).unwrap();
        assert_eq!(store.get(id).unwrap().root.key(), module.key());
        let rejected = runtime.memory.enter_execution(&runtime.resources).unwrap();
        let rejected_body = body(&mut runtime, &module, rejected);
        assert!(
            store
                .insert(
                    rejected,
                    module.clone(),
                    ExecutionOptions::default(),
                    ResourceCounters::default(),
                    runtime.modules.register_execution(&module).unwrap(),
                    rejected_body,
                )
                .is_err()
        );
        runtime
            .memory
            .retire_execution(&runtime.resources, rejected)
            .unwrap();
        assert!(store.remove(id).is_none());
        drop(frames);
        assert_eq!(store.records.borrow().slots.len(), 1);
        assert_eq!(store.frames.borrow().len(), 1);
        drop(store.remove(id).unwrap());
        runtime
            .memory
            .retire_execution(&runtime.resources, id)
            .unwrap();
        assert!(store.get(id).is_none());
        assert!(store.frames(id).is_none());
    }
}
