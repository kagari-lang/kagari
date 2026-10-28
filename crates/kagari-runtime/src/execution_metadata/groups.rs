//! Runtime-owned operation groups, addressed by checked identities.
use crate::module::ModuleStore;
use crate::{
    error::RuntimeError,
    execution_metadata::{
        groups::group::OperationGroup,
        operation::BoundOperation,
        scanning::{MetadataSweepCursor, RetirementBatch},
    },
    gc::{
        GcHeap,
        graph::{
            GraphIdentity, GraphMarker,
            marks::{Domain, MarkBlocks, MarkRef},
        },
    },
    native::storage::scan::{ScanBudget, ScanStatus},
};
use std::{cell::Ref, collections::TryReserveError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationGroupId {
    owner: u64,
    slot: usize,
    generation: u64,
}

/// A member ordinal within one checked, non-renumbering operation group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct OperationId {
    pub(crate) group: OperationGroupId,
    pub(crate) member: usize,
}

#[derive(Debug)]
struct Slot {
    generation: u64,
    group: Option<OperationGroup>,
}

#[derive(Debug)]
pub(crate) struct OperationGroupStore {
    owner: u64,
    slots: Vec<Slot>,
    marks: MarkBlocks,
    free: Vec<usize>,
    count: usize,
}

impl OperationGroupStore {
    pub(crate) fn new(owner: u64) -> Self {
        Self {
            owner,
            slots: Vec::new(),
            marks: MarkBlocks::default(),
            free: Vec::new(),
            count: 0,
        }
    }

    pub(crate) fn insert(
        &mut self,
        entries: Vec<BoundOperation>,
    ) -> Result<OperationGroupId, TryReserveError> {
        self.marks
            .prepare(self.free.last().copied().unwrap_or(self.slots.len()))?;
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.slots.try_reserve(1)?;
                self.free
                    .try_reserve(self.slots.len() + 1 - self.free.len())?;
                self.slots.push(Slot {
                    generation: 0,
                    group: None,
                });
                self.slots.len() - 1
            }
        };
        let record = &mut self.slots[slot];
        let id = OperationGroupId {
            owner: self.owner,
            slot,
            generation: record.generation,
        };
        record.group = Some(OperationGroup::new(entries, id));
        self.count += 1;
        Ok(id)
    }

    pub(crate) fn get(&self, id: OperationGroupId) -> Option<&OperationGroup> {
        if id.owner != self.owner {
            return None;
        }
        let slot = self.slots.get(id.slot)?;
        (slot.generation == id.generation)
            .then_some(slot.group.as_ref())
            .flatten()
    }

    pub(crate) fn operation(&self, id: OperationId) -> Option<&BoundOperation> {
        self.get(id.group)?.get(id)
    }

    pub(crate) fn mark(&self, id: OperationGroupId) -> Option<MarkRef<'_>> {
        self.get(id)?;
        Some(self.marks.slot(Domain::Group, id.slot))
    }

    pub(crate) fn next_gray(&self, block: usize, epoch: u64) -> (Option<GraphIdentity>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        let id = index.map(|index| {
            let slot = &self.slots[index];
            assert!(slot.group.is_some(), "gray owner is occupied");
            GraphIdentity::Group(OperationGroupId {
                owner: self.owner,
                slot: index,
                generation: slot.generation,
            })
        });
        (id, more)
    }

    pub(crate) fn count(&self) -> usize {
        self.count
    }

    pub(crate) fn begin_sweep(&self) -> MetadataSweepCursor {
        MetadataSweepCursor::new(self.owner, Domain::Group, self.slots.len())
    }

    /// Charge every inspected slot and transfer dead ownership without running destructors.
    pub(crate) fn detach_slice(
        &mut self,
        cursor: &mut MetadataSweepCursor,
        budget: &mut ScanBudget,
        limit: usize,
        graph: &mut GraphMarker<'_>,
        retired: &mut Vec<OperationGroup>,
    ) -> Result<ScanStatus, RuntimeError> {
        cursor.validate(self.owner, Domain::Group, self.slots.len(), graph.phase())?;
        let batch = RetirementBatch::new(retired.len(), limit)?;
        while cursor.next < cursor.end {
            if batch.is_full(retired.len()) || !budget.take(1) {
                return Ok(ScanStatus::Pending);
            }
            let index = cursor.next;
            let slot = &mut self.slots[index];
            let mark = self.marks.slot(Domain::Group, index);
            if slot.group.is_some() && !graph.is_survivor(mark)? {
                graph.can_retire(mark)?;
                batch.prepare(retired, cursor.remaining_slots())?;
                retired.push(slot.group.take().expect("occupied operation group"));
                graph.retire_prepared();
                self.count -= 1;
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

impl GcHeap {
    pub(crate) fn alloc_operation_group(
        &self,
        modules: &ModuleStore,
        entries: Vec<BoundOperation>,
    ) -> Result<OperationGroupId, RuntimeError> {
        self.ensure_execution_allowed()?;
        for operation in &entries {
            for field in 0..5 {
                if let Some(edge) = operation.scan_field(field) {
                    self.publish_metadata_edge(modules, edge)?;
                }
            }
        }
        let mut graph = self
            .graph
            .try_lock()
            .map_err(|_| RuntimeError::module_validation("graph admission is borrowed"))?;
        graph.prepare_identity()?;
        let id = self
            .operation_groups
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("operation group store is borrowed"))?
            .insert(entries)
            .map_err(|_| self.resource_limit("operation group storage"))?;
        self.with_heap_mark(GraphIdentity::Group(id), |mark| {
            graph.admit_prepared(GraphIdentity::Group(id), mark);
            Ok(())
        })?;
        Ok(id)
    }

    pub(crate) fn alloc_bound_operation(
        &self,
        modules: &ModuleStore,
        operation: BoundOperation,
    ) -> Result<OperationId, RuntimeError> {
        let group = self.alloc_operation_group(modules, vec![operation])?;
        Ok(OperationId { group, member: 0 })
    }

    pub(crate) fn bound_operation(&self, id: OperationId) -> Option<Ref<'_, BoundOperation>> {
        if !self.heap_identity_accessible(GraphIdentity::Group(id.group)) {
            return None;
        }
        Ref::filter_map(self.operation_groups.try_borrow().ok()?, |store| {
            store.operation(id)
        })
        .ok()
    }

    pub(crate) fn operation_group(&self, id: OperationGroupId) -> Option<Ref<'_, OperationGroup>> {
        if !self.heap_identity_accessible(GraphIdentity::Group(id)) {
            return None;
        }
        Ref::filter_map(self.operation_groups.try_borrow().ok()?, |store| {
            store.get(id)
        })
        .ok()
    }
}

mod group;

#[cfg(test)]
mod tests;
