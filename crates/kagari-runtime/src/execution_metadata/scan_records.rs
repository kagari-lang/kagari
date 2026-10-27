//! Stable runtime-local identities for shared executable records, never Arc addresses.
use crate::{
    error::RuntimeError,
    execution_metadata::{MetadataEdge, call_contracts::ScopedInterfaceCall},
    gc::{
        GcHeap,
        graph::{
            GraphIdentity, GraphMarker, GraphState,
            marks::{Domain, MarkBlocks, MarkRef},
        },
        interfaces::InterfaceDescription,
    },
    module::ModuleStore,
    native::application::NativeApplication,
};
use std::{marker::PhantomData, sync::Arc};

pub(crate) trait ScanRecordId: Copy {
    const DOMAIN: Domain;

    fn from_parts(owner: u64, slot: usize, generation: u64) -> Self;

    fn parts(self) -> (u64, usize, u64);
}

macro_rules! record_id {
    ($name:ident, $domain:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub(crate) struct $name {
            owner: u64,
            slot: usize,
            generation: u64,
        }

        impl ScanRecordId for $name {
            const DOMAIN: Domain = Domain::$domain;

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

record_id!(NativeApplicationScanId, NativeApplication);
record_id!(InterfaceCallScanId, InterfaceCall);
record_id!(InterfaceDescriptionScanId, InterfaceDescription);

#[derive(Debug)]
struct Slot<T> {
    generation: u64,

    value: Option<Arc<T>>,
}

/// Physical retention is not a graph root. A record remains resolvable while its
/// pending/active work exists, even if a cache or semantic owner releases its Arc.
#[derive(Debug)]
pub(crate) struct SharedScanStore<T, I> {
    owner: u64,
    slots: Vec<Slot<T>>,
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

    /// Physical registry retention and external Arc clones are not graph roots.
    /// Return backing for disposal after the store borrow and graph lock end.
    pub(crate) fn detach_batch(
        &mut self,
        cursor: &mut usize,
        limit: usize,
        graph: &mut GraphMarker,
    ) -> Vec<Arc<T>> {
        assert!(limit != 0);
        let mut retired = Vec::new();
        while *cursor < self.slots.len() && retired.len() < limit {
            let index = *cursor;
            *cursor += 1;
            let slot = &mut self.slots[index];
            if slot.value.is_none()
                || graph
                    .is_survivor(self.marks.slot(I::DOMAIN, index))
                    .expect("closed shared marks")
            {
                continue;
            }
            graph
                .can_retire(self.marks.slot(I::DOMAIN, index))
                .expect("closed shared record work");
            graph.retire_prepared();
            retired.push(slot.value.take().expect("occupied shared record"));
            if let Some(generation) = slot.generation.checked_add(1) {
                slot.generation = generation;
                self.free.push(index);
            }
        }
        retired
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
    use crate::native::storage::scan::ScanBudget;

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
        assert!(graph.charge_bitmap_work(&mut ScanBudget::new(4)));
        graph.close_mark().unwrap();
        let mut cursor = 0;
        for (count, position) in [(32, 32), (32, 65), (6, 71), (0, 71)] {
            let retired = store.detach_batch(&mut cursor, 32, &mut graph);
            assert_eq!(retired.len(), count);
            assert_eq!(cursor, position);
            assert_eq!(store.get(ids[35]), Some(&7));
            drop(retired);
        }
        for (index, id) in ids.iter().enumerate() {
            assert_eq!(store.get(*id).is_some(), index == 35);
        }
        graph.finish_sweep().unwrap();
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
        assert!(graph.charge_bitmap_work(&mut ScanBudget::new(4)));
        graph.close_mark().unwrap();
        assert!(store.detach_batch(&mut 0, 32, &mut graph).is_empty());
        assert_eq!(store.get(first), Some(&7));
        graph.finish_sweep().unwrap();
        graph.begin_cycle().unwrap();
        graph.finish_roots().unwrap();
        graph.close_mark().unwrap();
        assert_eq!(store.detach_batch(&mut 0, 32, &mut graph).len(), 1);
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
