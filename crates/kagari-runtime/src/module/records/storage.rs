//! Module records own stable scan slots; the key map is only a lookup index.
#[cfg(feature = "execution-diagnostics")]
use crate::diagnostics::storage::BufferUsage;
use crate::{
    error::RuntimeError,
    gc::graph::{
        GraphIdentity, GraphMarker, GraphState,
        marks::{Domain, MarkBlocks, MarkRef},
    },
    module::{ExecutableModule, ModuleKey, records::ModuleRecord, scanning::ProgramScanId},
};
use std::{collections::HashMap, ops::Index};

#[derive(Debug)]
struct Slot {
    generation: u64,
    // Identity history outlives retired executable backing. Keeping the large
    // record inline would retain every vacant reload slot's full record footprint.
    record: Option<Box<ModuleRecord>>,
}

/// A checked occupied slot, valid only within its original store borrow. Ordinary
/// access carries two words, not an eagerly constructed scan identity and mark.
#[derive(Clone, Copy)]
pub(in crate::module) struct ResolvedRecord<'a> {
    slot: &'a Slot,
    index: usize,
}

impl<'a> ResolvedRecord<'a> {
    pub(in crate::module) fn record(self) -> &'a ModuleRecord {
        self.slot
            .record
            .as_deref()
            .expect("resolved occupied module")
    }

    pub(in crate::module) fn identity(self) -> ProgramScanId {
        ProgramScanId {
            owner: self.record().module.registry_owner,
            slot: self.index,
            generation: self.slot.generation,
        }
    }
}

#[derive(Debug, Default)]
pub(in crate::module) struct ModuleRecords {
    by_key: HashMap<ModuleKey, usize>,
    slots: Vec<Slot>,
    marks: MarkBlocks,
    free: Vec<usize>,
}

impl ModuleRecords {
    #[cfg(feature = "execution-diagnostics")]
    pub(in crate::module) fn storage_usage(&self) -> BufferUsage {
        BufferUsage::new("module_slots", &self.slots)
    }

    pub(in crate::module) fn scan_extent(&self) -> usize {
        self.slots.len()
    }

    pub(in crate::module) fn scan_slot(
        &self,
        index: usize,
    ) -> Option<(ProgramScanId, &ModuleRecord)> {
        let slot = self.slots.get(index)?;
        let record = slot.record.as_deref()?;
        Some((
            ProgramScanId {
                owner: record.module.registry_owner,
                slot: index,
                generation: slot.generation,
            },
            record,
        ))
    }

    pub(in crate::module) fn len(&self) -> usize {
        self.by_key.len()
    }

    pub(in crate::module) fn keys(&self) -> impl Iterator<Item = &ModuleKey> {
        self.by_key.keys()
    }

    pub(in crate::module) fn get(&self, key: &ModuleKey) -> Option<&ModuleRecord> {
        self.slots.get(*self.by_key.get(key)?)?.record.as_deref()
    }

    pub(in crate::module) fn resolve(&self, key: &ModuleKey) -> Option<ResolvedRecord<'_>> {
        self.resolve_slot(*self.by_key.get(key)?)
    }

    pub(in crate::module) fn resolve_slot(&self, index: usize) -> Option<ResolvedRecord<'_>> {
        let slot = self.slots.get(index)?;
        slot.record.as_ref()?;
        Some(ResolvedRecord { slot, index })
    }

    /// Both views come from this same short store borrow; no graph view escapes.
    pub(in crate::module) fn accessible(
        &self,
        resolved: ResolvedRecord<'_>,
        graph: &GraphState,
    ) -> Result<bool, RuntimeError> {
        graph
            .with_access(|| self.marks.slot(Domain::Program, resolved.index))
            .map_err(RuntimeError::from)
    }

    pub(in crate::module) fn publish(
        &self,
        resolved: ResolvedRecord<'_>,
        graph: &GraphState,
    ) -> Result<(), RuntimeError> {
        graph.with_publication(|| {
            (
                GraphIdentity::Program(resolved.identity()),
                self.marks.slot(Domain::Program, resolved.index),
            )
        })
    }

    pub(in crate::module) fn get_mut(&mut self, key: &ModuleKey) -> Option<&mut ModuleRecord> {
        self.slots
            .get_mut(*self.by_key.get(key)?)?
            .record
            .as_deref_mut()
    }

    #[cfg(test)]
    pub(in crate::module) fn contains_key(&self, key: &ModuleKey) -> bool {
        self.by_key.contains_key(key)
    }

    pub(in crate::module) fn values(&self) -> impl Iterator<Item = &ModuleRecord> {
        self.slots.iter().filter_map(|slot| slot.record.as_deref())
    }

    pub(in crate::module) fn scan_id(&self, module: &ExecutableModule) -> Option<ProgramScanId> {
        let index = *self.by_key.get(&module.key())?;
        let slot = self.slots.get(index)?;
        slot.record
            .as_ref()
            .filter(|record| record.matches(module))?;
        Some(ProgramScanId {
            owner: module.registry_owner,
            slot: index,
            generation: slot.generation,
        })
    }

    pub(in crate::module) fn scan_record(&self, id: ProgramScanId) -> Option<&ModuleRecord> {
        let slot = self.slots.get(id.slot)?;
        let record = slot.record.as_deref()?;
        (slot.generation == id.generation && record.module.registry_owner == id.owner)
            .then_some(record)
    }

    pub(in crate::module) fn mark(&self, id: ProgramScanId) -> Option<MarkRef<'_>> {
        self.scan_record(id)?;
        Some(self.marks.slot(Domain::Program, id.slot))
    }

    pub(in crate::module) fn next_gray(
        &self,
        block: usize,
        epoch: u64,
    ) -> (Option<GraphIdentity>, bool) {
        let (index, more) = self.marks.next_gray(block, epoch);
        (
            index.map(|index| {
                GraphIdentity::Program(self.scan_slot(index).expect("gray program record").0)
            }),
            more,
        )
    }

    pub(in crate::module) fn scan_record_mut(
        &mut self,
        id: ProgramScanId,
    ) -> Option<&mut ModuleRecord> {
        self.scan_record(id)?;
        self.slots.get_mut(id.slot)?.record.as_deref_mut()
    }

    pub(in crate::module) fn insert(
        &mut self,
        record: ModuleRecord,
        graph: &mut GraphMarker,
    ) -> Result<(), RuntimeError> {
        let key = record.module.key();
        if self.by_key.contains_key(&key) {
            return Err(RuntimeError::module_validation(
                "duplicate module scan record",
            ));
        }
        graph.prepare_identity()?;
        self.by_key
            .try_reserve(1)
            .map_err(|_| RuntimeError::resource_limit("module record index"))?;
        if self.free.is_empty() {
            self.slots
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("module scan slots"))?;
            self.free
                .try_reserve(self.slots.len() + 1 - self.free.len())
                .map_err(|_| RuntimeError::resource_limit("module slot retirement"))?;
        }
        let index = self.free.last().copied().unwrap_or(self.slots.len());
        self.marks
            .prepare(index)
            .map_err(|_| RuntimeError::resource_limit("module block marks"))?;
        let generation = self.slots.get(index).map_or(0, |slot| slot.generation);
        let id = ProgramScanId {
            owner: record.module.registry_owner,
            slot: index,
            generation,
        };
        let record = Box::new(record);
        if index == self.slots.len() {
            self.slots.push(Slot {
                generation,
                record: Some(record),
            });
        } else {
            self.free.pop();
            self.slots[index].record = Some(record);
        }
        self.by_key.insert(key, index);
        graph.admit_prepared(
            GraphIdentity::Program(id),
            self.marks.slot(Domain::Program, index),
        );
        Ok(())
    }

    pub(in crate::module) fn remove(
        &mut self,
        key: &ModuleKey,
        graph: &mut GraphMarker,
    ) -> Option<Box<ModuleRecord>> {
        let index = *self.by_key.get(key)?;
        let slot = &mut self.slots[index];
        graph
            .can_retire(self.marks.slot(Domain::Program, index))
            .ok()?;
        let record = slot.record.take()?;
        self.by_key.remove(key);
        graph.retire_prepared();
        if let Some(generation) = slot.generation.checked_add(1) {
            slot.generation = generation;
            self.free.push(index);
        }
        Some(record)
    }
}

impl Index<&ModuleKey> for ModuleRecords {
    type Output = ModuleRecord;

    fn index(&self, key: &ModuleKey) -> &Self::Output {
        self.get(key).expect("module record key")
    }
}
