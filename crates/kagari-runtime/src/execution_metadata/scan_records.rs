//! Stable runtime-local identities for shared executable records, never Arc addresses.
use crate::{
    error::RuntimeError,
    execution_metadata::{
        MetadataEdge,
        call_contracts::ScopedInterfaceCall,
        interfaces::InterfaceDescriptionRelease,
        scanning::{MetadataSweepCursor, RetirementBatch},
    },
    gc::{
        GcHeap,
        graph::{
            GraphIdentity, GraphMarker, GraphState,
            marks::{Domain, MarkBlocks, MarkRef},
        },
        interfaces::InterfaceDescription,
    },
    module::ModuleStore,
    native::{
        application::NativeApplication,
        storage::scan::{ScanBudget, ScanStatus},
    },
};
use std::{fmt::Debug, marker::PhantomData, mem, sync::Arc};

pub(crate) trait ScanRecordId: Copy {
    const DOMAIN: Domain;
    type CacheRelease: Default + Debug;

    fn from_parts(owner: u64, slot: usize, generation: u64) -> Self;

    fn parts(self) -> (u64, usize, u64);
}

macro_rules! record_id {
    ($name:ident, $domain:ident, $key:ty) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $name {
            owner: u64,
            slot: usize,
            generation: u64,
        }

        impl ScanRecordId for $name {
            const DOMAIN: Domain = Domain::$domain;
            type CacheRelease = $key;

            fn from_parts(owner: u64, slot: usize, generation: u64) -> Self {
                Self {
                    owner,
                    slot,
                    generation,
                }
            }

            fn parts(self) -> (u64, usize, u64) {
                (self.owner, self.slot, self.generation)
            }
        }
    };
}

record_id!(NativeApplicationScanId, NativeApplication, ());
record_id!(InterfaceCallScanId, InterfaceCall, ());
record_id!(
    InterfaceDescriptionScanId,
    InterfaceDescription,
    Option<InterfaceDescriptionRelease>
);

#[derive(Debug)]
struct Slot<T, K> {
    generation: u64,
    value: Option<Arc<T>>,
    cache_release: K,
}

/// One detached owner, including its original cache-release identity. This is
/// caller-owned even on partial sweep failure; drop only after all store borrows end.
#[derive(Debug)]
pub(crate) struct RetiredScanRecord<T, I: ScanRecordId> {
    pub(crate) id: I,
    pub(crate) record: Arc<T>,
    pub(crate) cache_release: I::CacheRelease,
}

/// Physical retention is not a graph root. A record remains resolvable while its
/// pending/active work exists, even if a cache or semantic owner releases its Arc.
#[derive(Debug)]
pub(crate) struct SharedScanStore<T, I: ScanRecordId> {
    owner: u64,
    slots: Vec<Slot<T, I::CacheRelease>>,
    marks: MarkBlocks,
    free: Vec<usize>,
    identity: PhantomData<I>,
}

impl<T, I: ScanRecordId> SharedScanStore<T, I> {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            marks: MarkBlocks::default(),
            free: Vec::new(),
            identity: PhantomData,
        }
    }

    pub(crate) fn insert_with(
        &mut self,
        state: &GraphState,
        identity: fn(I) -> GraphIdentity,
        build: impl FnOnce(I) -> T,
        validate: impl FnOnce(&T) -> Result<(), RuntimeError>,
    ) -> Result<Arc<T>, RuntimeError> {
        state.try_lock()?.prepare_identity()?;
        if self.free.is_empty() {
            self.slots
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("shared scan records"))?;
            self.free
                .try_reserve(self.slots.len() + 1 - self.free.len())
                .map_err(|_| RuntimeError::resource_limit("shared scan record retirement"))?;
        }
        let index = self.free.last().copied().unwrap_or(self.slots.len());
        self.marks
            .prepare(index)
            .map_err(|_| RuntimeError::resource_limit("shared record marks"))?;
        let generation = self.slots.get(index).map_or(0, |slot| slot.generation);
        let id = I::from_parts(self.owner, index, generation);
        let value = Arc::new(build(id));
        // The candidate has no exposed identity. No graph lock spans its edge
        // checks; those can resolve other owners and shade their reserved work.
        validate(&value)?;
        let mut graph = state.try_lock()?;
        // Preparation is exclusive with commit, even if construction admitted
        // another owner. The store borrow prevents reuse of this candidate slot.
        graph.prepare_identity()?;
        if index == self.slots.len() {
            self.slots.push(Slot {
                generation,
                value: Some(value.clone()),
                cache_release: Default::default(),
            });
        } else {
            self.free.pop();
            self.slots[index].value = Some(value.clone());
        }
        graph.admit_prepared(identity(id), self.marks.slot(I::DOMAIN, index));
        Ok(value)
    }

    pub(crate) fn get(&self, id: I) -> Option<&T> {
        let (owner, index, generation) = id.parts();
        if owner != self.owner {
            return None;
        }
        let slot = self.slots.get(index)?;
        (slot.generation == generation)
            .then_some(slot.value.as_deref())
            .flatten()
    }

    pub(crate) fn mark(&self, id: I) -> Option<MarkRef<'_>> {
        self.get(id)?;
        Some(self.marks.slot(I::DOMAIN, id.parts().1))
    }

    pub(crate) fn next_gray(&self, block: usize, epoch: u64) -> (Option<I>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        (
            index.map(|index| {
                let slot = &self.slots[index];
                assert!(slot.value.is_some(), "gray shared owner is retained");
                I::from_parts(self.owner, index, slot.generation)
            }),
            more,
        )
    }

    pub(crate) fn begin_sweep(&self) -> MetadataSweepCursor {
        MetadataSweepCursor::new(self.owner, I::DOMAIN, self.slots.len())
    }

    /// Registry retention and external Arc clones are not graph roots. Charge
    /// live/vacant slots too, and leave all detached backing with the caller.
    pub(crate) fn detach_slice(
        &mut self,
        cursor: &mut MetadataSweepCursor,
        budget: &mut ScanBudget,
        limit: usize,
        graph: &mut GraphMarker<'_>,
        retired: &mut Vec<RetiredScanRecord<T, I>>,
    ) -> Result<ScanStatus, RuntimeError> {
        cursor.validate(self.owner, I::DOMAIN, self.slots.len(), graph.phase())?;
        let batch = RetirementBatch::new(retired.len(), limit)?;
        while cursor.next < cursor.end {
            if batch.is_full(retired.len()) || !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let index = cursor.next;
            let slot = &mut self.slots[index];
            let mark = self.marks.slot(I::DOMAIN, index);
            if slot.value.is_some() && !graph.is_survivor(mark)? {
                graph.can_retire(mark)?;
                batch.prepare(retired, cursor.remaining_slots())?;
                retired.push(RetiredScanRecord {
                    id: I::from_parts(self.owner, index, slot.generation),
                    record: slot.value.take().expect("occupied shared record"),
                    cache_release: mem::take(&mut slot.cache_release),
                });
                graph.retire_prepared();
                if let Some(generation) = slot.generation.checked_add(1) {
                    slot.generation = generation;
                    self.free.push(index);
                }
            }
            cursor.next += 1;
        }
        Ok(ScanStatus::Complete)
    }
}

impl SharedScanStore<InterfaceDescription, InterfaceDescriptionScanId> {
    /// Attach release metadata to the original scan record, never a second registry.
    pub(super) fn cache_release(
        &mut self,
        id: InterfaceDescriptionScanId,
        release: InterfaceDescriptionRelease,
    ) -> Result<Option<InterfaceDescriptionRelease>, RuntimeError> {
        self.get(id)
            .ok_or_else(|| RuntimeError::module_validation("invalid description cache owner"))?;
        let slot = &mut self.slots[id.slot];
        if !release.is_for(slot.value.as_ref().expect("validated description")) {
            return Err(RuntimeError::module_validation(
                "foreign description cache release",
            ));
        }
        if let Some(original) = &slot.cache_release
            && original.key() != release.key()
        {
            return Err(RuntimeError::module_validation(
                "description cache key changed",
            ));
        }
        Ok(slot.cache_release.replace(release))
    }
}

impl GcHeap {
    fn publish_scan_fields<'a>(
        &self,
        modules: &ModuleStore,
        len: usize,
        field: impl Fn(usize) -> Option<MetadataEdge<'a>>,
    ) -> Result<(), RuntimeError> {
        for index in 0..len {
            if let Some(edge) = field(index) {
                self.publish_metadata_edge(modules, edge)?;
            }
        }
        Ok(())
    }

    pub(crate) fn admit_native_application(
        &self,
        modules: &ModuleStore,
        build: impl FnOnce(NativeApplicationScanId) -> NativeApplication,
    ) -> Result<Arc<NativeApplication>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.native_applications
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("native application store is borrowed"))?
            .insert_with(
                &self.graph,
                GraphIdentity::NativeApplication,
                build,
                |record| {
                    self.publish_scan_fields(modules, record.scan_len(), |i| record.scan_field(i))
                },
            )
    }

    pub(crate) fn admit_interface_call(
        &self,
        modules: &ModuleStore,
        build: impl FnOnce(InterfaceCallScanId) -> ScopedInterfaceCall,
    ) -> Result<Arc<ScopedInterfaceCall>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.interface_calls
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("interface call store is borrowed"))?
            .insert_with(&self.graph, GraphIdentity::InterfaceCall, build, |record| {
                self.publish_scan_fields(modules, record.scan_len(), |i| record.scan_field(i))
            })
    }

    pub(crate) fn admit_interface_description(
        &self,
        modules: &ModuleStore,
        build: impl FnOnce(InterfaceDescriptionScanId) -> InterfaceDescription,
    ) -> Result<Arc<InterfaceDescription>, RuntimeError> {
        self.ensure_execution_allowed()?;
        self.interface_descriptions
            .try_borrow_mut()
            .map_err(|_| {
                RuntimeError::module_validation("interface description store is borrowed")
            })?
            .insert_with(
                &self.graph,
                GraphIdentity::InterfaceDescription,
                build,
                |record| {
                    self.publish_scan_fields(modules, record.scan_len(), |i| record.scan_field(i))
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        panic::{AssertUnwindSafe, catch_unwind},
        rc::{Rc, Weak},
    };

    fn byte_record(
        store: &mut SharedScanStore<u8, InterfaceCallScanId>,
        state: &GraphState,
    ) -> InterfaceCallScanId {
        let mut id = None;
        store
            .insert_with(
                state,
                GraphIdentity::InterfaceCall,
                |slot| {
                    id = Some(slot);
                    7
                },
                |_| Ok(()),
            )
            .unwrap();
        id.unwrap()
    }

    fn empty_cycle(state: &GraphState) {
        let mut graph = state.lock();
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
    }

    #[test]
    fn finite_sweep_extent_keeps_newborns_and_reused_slots_on_both_sides_of_cursor() {
        let state = GraphState::default();
        let mut store = SharedScanStore::<u8, InterfaceCallScanId>::new(1);
        let old = (0..4)
            .map(|_| byte_record(&mut store, &state))
            .collect::<Vec<_>>();
        empty_cycle(&state);
        let mut cursor = store.begin_sweep();
        let mut retired = Vec::new();
        let mut zero = ScanBudget::new(0);
        assert_eq!(
            store
                .detach_slice(&mut cursor, &mut zero, 32, &mut state.lock(), &mut retired)
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(cursor.next, 0);
        assert!(retired.is_empty());
        assert_eq!(retired.capacity(), 0);
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut ScanBudget::new(2),
                    32,
                    &mut state.lock(),
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(retired.len(), 2);
        let mut allowance_budget = ScanBudget::new(10);
        assert!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut allowance_budget,
                    31,
                    &mut state.lock(),
                    &mut retired
                )
                .is_err()
        );
        assert_eq!(cursor.next, 2);
        assert_eq!(allowance_budget.spent(), 0);
        assert_eq!(retired.len(), 2);
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut allowance_budget,
                    0,
                    &mut state.lock(),
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(cursor.next, 2);
        assert_eq!(allowance_budget.spent(), 0);
        assert_eq!(retired.len(), 2);
        retired.clear();
        let behind = [
            byte_record(&mut store, &state),
            byte_record(&mut store, &state),
        ];
        assert!(behind.iter().all(|id| id.slot < cursor.next));
        let appended = byte_record(&mut store, &state);
        assert_eq!(appended.slot, cursor.end);
        let mut budget = ScanBudget::new(20);
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut budget,
                    32,
                    &mut state.lock(),
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert_eq!(budget.spent(), 2);
        assert_eq!(retired.len(), 2);
        assert!(old.iter().all(|id| store.get(*id).is_none()));
        assert!(
            behind
                .iter()
                .chain([&appended])
                .all(|id| store.get(*id).is_some())
        );
        retired.clear();
        state.lock().finish_sweep().unwrap();

        empty_cycle(&state);
        let mut cursor = store.begin_sweep();
        let ahead = [
            byte_record(&mut store, &state),
            byte_record(&mut store, &state),
        ];
        assert!(
            ahead
                .iter()
                .all(|id| id.slot > cursor.next && id.slot < cursor.end)
        );
        let appended = byte_record(&mut store, &state);
        assert_eq!(appended.slot, cursor.end);
        let mut budget = ScanBudget::new(20);
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut budget,
                    32,
                    &mut state.lock(),
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert_eq!(budget.spent(), 5);
        assert_eq!(retired.len(), 3);
        assert!(
            ahead
                .iter()
                .chain([&appended])
                .all(|id| store.get(*id).is_some())
        );
        state.lock().finish_sweep().unwrap();
    }

    #[test]
    fn gray_shared_record_survives_eviction_and_rejects_sweep_until_scan_finishes() {
        let state = GraphState::default();
        let mut store = SharedScanStore::<u8, InterfaceCallScanId>::new(1);
        let mut id = None;
        let cache = store
            .insert_with(
                &state,
                GraphIdentity::InterfaceCall,
                |slot| {
                    id = Some(slot);
                    7
                },
                |_| Ok(()),
            )
            .unwrap();
        let weak = Arc::downgrade(&cache);
        let id = id.unwrap();
        let mut graph = state.lock();
        graph.begin_cycle().unwrap();
        graph
            .shade(GraphIdentity::InterfaceCall(id), store.mark(id).unwrap())
            .unwrap();
        let active = graph
            .next_owner(|group, epoch| {
                let (id, more) = store.next_gray(group.block, epoch);
                Ok((id.map(GraphIdentity::InterfaceCall), more))
            })
            .unwrap()
            .unwrap();
        drop(cache);
        assert!(
            weak.upgrade().is_some(),
            "the original scan record is a strong physical owner"
        );
        let mut retired = Vec::new();
        let mut cursor = store.begin_sweep();
        assert!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut ScanBudget::new(1),
                    32,
                    &mut graph,
                    &mut retired
                )
                .is_err()
        );
        assert!(retired.is_empty());
        assert_eq!(retired.capacity(), 0);
        assert_eq!(cursor.next, 0);
        assert!(graph.can_retire(store.mark(id).unwrap()).is_err());
        graph.finish_owner(active, store.mark(id).unwrap()).unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut ScanBudget::new(1),
                    32,
                    &mut graph,
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert!(retired.is_empty());
        assert_eq!(
            retired.capacity(),
            0,
            "a survivor needs no disposal backing"
        );
        graph.finish_sweep().unwrap();
    }

    #[derive(Debug)]
    struct DropProbe {
        store: Weak<RefCell<SharedScanStore<DropProbe, InterfaceCallScanId>>>,
        graph: Arc<GraphState>,
        drops: Rc<Cell<usize>>,
        panic: bool,
    }

    impl Drop for DropProbe {
        fn drop(&mut self) {
            assert!(self.store.upgrade().unwrap().try_borrow_mut().is_ok());
            assert!(self.graph.try_lock().is_ok());
            self.drops.set(self.drops.get() + 1);
            assert!(!self.panic, "injected retired metadata destructor panic");
        }
    }

    #[test]
    fn detached_outputs_remain_owned_across_destructor_panic_and_release_borrows_first() {
        let graph = Arc::new(GraphState::default());
        let store = Rc::new(RefCell::new(SharedScanStore::<
            DropProbe,
            InterfaceCallScanId,
        >::new(1)));
        let drops = Rc::new(Cell::new(0));
        for panic in [true, false] {
            store
                .borrow_mut()
                .insert_with(
                    &graph,
                    GraphIdentity::InterfaceCall,
                    |_| DropProbe {
                        store: Rc::downgrade(&store),
                        graph: graph.clone(),
                        drops: drops.clone(),
                        panic,
                    },
                    |_| Ok(()),
                )
                .unwrap();
        }
        empty_cycle(&graph);
        let mut cursor = store.borrow().begin_sweep();
        let mut retired = Vec::new();
        let status = store
            .borrow_mut()
            .detach_slice(
                &mut cursor,
                &mut ScanBudget::new(2),
                32,
                &mut graph.lock(),
                &mut retired,
            )
            .unwrap();
        assert_eq!(status, ScanStatus::Complete);
        let mut foreign = MetadataSweepCursor::new(2, Domain::InterfaceCall, cursor.end);
        let mut budget = ScanBudget::new(2);
        assert!(
            store
                .borrow_mut()
                .detach_slice(
                    &mut foreign,
                    &mut budget,
                    30,
                    &mut graph.lock(),
                    &mut retired,
                )
                .is_err()
        );
        assert_eq!(budget.spent(), 0);
        assert_eq!(foreign.next, 0);
        assert_eq!(retired.len(), 2);
        assert_eq!(drops.get(), 0);
        let first = retired.remove(0);
        assert!(catch_unwind(AssertUnwindSafe(|| drop(first))).is_err());
        assert_eq!(drops.get(), 1);
        assert_eq!(retired.len(), 1);
        drop(retired);
        assert_eq!(drops.get(), 2);
        let mut again = Vec::new();
        let mut holes = store.borrow().begin_sweep();
        assert_eq!(
            store
                .borrow_mut()
                .detach_slice(
                    &mut holes,
                    &mut ScanBudget::new(2),
                    32,
                    &mut graph.lock(),
                    &mut again
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert!(again.is_empty());
        assert_eq!(again.capacity(), 0, "vacancies need no disposal backing");
        graph.lock().finish_sweep().unwrap();
    }

    #[test]
    fn retirement_batches_keep_survivors_and_full_generations_without_rescanning() {
        let state = GraphState::default();
        let mut store = SharedScanStore::<u8, InterfaceCallScanId>::new(1);
        let mut ids = Vec::new();
        for _ in 0..71 {
            store
                .insert_with(
                    &state,
                    GraphIdentity::InterfaceCall,
                    |id| {
                        ids.push(id);
                        7
                    },
                    |_| Ok(()),
                )
                .unwrap();
        }
        let mut graph = state.lock();
        graph.begin_cycle().unwrap();
        graph
            .shade(
                GraphIdentity::InterfaceCall(ids[35]),
                store.mark(ids[35]).unwrap(),
            )
            .unwrap();
        let owner = graph
            .next_owner(|group, epoch| {
                let (id, more) = store.next_gray(group.block, epoch);
                Ok((id.map(GraphIdentity::InterfaceCall), more))
            })
            .unwrap()
            .unwrap();
        graph
            .finish_owner(owner, store.mark(ids[35]).unwrap())
            .unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        let mut cursor = store.begin_sweep();
        for (count, position, work, status) in [
            (32, 32, 32, ScanStatus::Pending),
            (31, 64, 32, ScanStatus::Pending),
            (7, 71, 7, ScanStatus::Complete),
            (0, 71, 0, ScanStatus::Complete),
        ] {
            let mut retired = Vec::new();
            let mut budget = ScanBudget::new(32);
            assert_eq!(
                store
                    .detach_slice(&mut cursor, &mut budget, 32, &mut graph, &mut retired)
                    .unwrap(),
                status
            );
            assert_eq!(retired.len(), count);
            if count == 0 {
                assert_eq!(retired.capacity(), 0);
            }
            assert_eq!(cursor.next, position);
            assert_eq!(budget.spent(), work);
            assert_eq!(store.get(ids[35]), Some(&7));
            drop(retired);
        }
        let mut holes = store.begin_sweep();
        let mut budget = ScanBudget::new(3);
        let mut retired = Vec::new();
        assert_eq!(
            store
                .detach_slice(&mut holes, &mut budget, 32, &mut graph, &mut retired)
                .unwrap(),
            ScanStatus::Pending
        );
        assert_eq!(holes.next, 3);
        assert_eq!(budget.spent(), 3);
        assert!(retired.is_empty());
        assert_eq!(retired.capacity(), 0);
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(store.get(*id).is_some(), index == 35);
        }
        let mut no_work = ScanBudget::new(0);
        assert!(
            store
                .detach_slice(&mut cursor, &mut no_work, 33, &mut graph, &mut retired)
                .is_err(),
            "an exhausted cursor still validates its retirement allowance"
        );
        let mut foreign = MetadataSweepCursor::new(2, Domain::InterfaceCall, 0);
        assert!(
            store
                .detach_slice(&mut foreign, &mut no_work, 32, &mut graph, &mut retired)
                .is_err(),
            "an empty cursor still validates its owner"
        );
        graph.finish_sweep().unwrap();
        assert!(
            store
                .detach_slice(&mut cursor, &mut no_work, 32, &mut graph, &mut retired)
                .is_err(),
            "an exhausted cursor still validates the sweep phase"
        );
        assert_eq!(cursor.next, 71);
        assert_eq!(no_work.spent(), 0);
        assert_eq!(retired.capacity(), 0);
    }

    #[test]
    fn shared_records_keep_one_identity_and_reject_reused_or_foreign_slots() {
        let state = GraphState::default();
        let mut store = SharedScanStore::<u8, InterfaceCallScanId>::new(1);
        let mut first = None;
        let value = store
            .insert_with(
                &state,
                GraphIdentity::InterfaceCall,
                |id| {
                    first = Some(id);
                    7
                },
                |_| Ok(()),
            )
            .unwrap();
        let first = first.unwrap();
        let clone = value.clone();
        drop(value);
        let mut graph = state.lock();
        graph.begin_cycle().unwrap();
        graph
            .shade(
                GraphIdentity::InterfaceCall(first),
                store.mark(first).unwrap(),
            )
            .unwrap();
        assert_eq!(
            graph
                .next_owner(|group, epoch| {
                    let (id, more) = store.next_gray(group.block, epoch);
                    Ok((id.map(GraphIdentity::InterfaceCall), more))
                })
                .unwrap(),
            Some(GraphIdentity::InterfaceCall(first))
        );
        graph
            .finish_owner(
                GraphIdentity::InterfaceCall(first),
                store.mark(first).unwrap(),
            )
            .unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        let mut cursor = store.begin_sweep();
        let mut retired = Vec::new();
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut ScanBudget::new(1),
                    32,
                    &mut graph,
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert!(retired.is_empty());
        assert_eq!(store.get(first), Some(&7));
        graph.finish_sweep().unwrap();
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        let mut cursor = store.begin_sweep();
        assert_eq!(
            store
                .detach_slice(
                    &mut cursor,
                    &mut ScanBudget::new(1),
                    32,
                    &mut graph,
                    &mut retired
                )
                .unwrap(),
            ScanStatus::Complete
        );
        assert_eq!(retired.len(), 1);
        assert_eq!(retired[0].id, first);
        assert_eq!(*retired[0].record, 7);
        drop(retired);
        assert_eq!(*clone, 7, "an Arc retains backing, not graph admission");
        drop(clone);
        graph.finish_sweep().unwrap();
        assert!(store.get(first).is_none());
        drop(graph);
        let mut second = None;
        let _value = store
            .insert_with(
                &state,
                GraphIdentity::InterfaceCall,
                |id| {
                    second = Some(id);
                    9
                },
                |_| Ok(()),
            )
            .unwrap();
        let second = second.unwrap();
        assert_eq!(first.slot, second.slot);
        assert_ne!(first.generation, second.generation);
        assert!(store.get(first).is_none());
        assert!(
            store
                .get(InterfaceCallScanId { owner: 2, ..second })
                .is_none()
        );
        assert_eq!(store.get(second), Some(&9));
    }
}
