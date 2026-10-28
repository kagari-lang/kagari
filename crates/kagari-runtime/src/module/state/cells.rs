//! Allocation and transactional generation bindings. The module store owns cells;
//! immutable loaded descriptors and prepared programs never own mutable state.
use crate::{
    error::RuntimeError,
    frame::types::compatibility::TypeView,
    gc::graph::{GraphIdentity, GraphMarker, GraphState, marks::OwnerMark},
    module::{
        ExecutableModule, InstallationId, ModuleStore, ModuleStoreInner,
        records::ModuleRecord,
        scanning::StaticScanId,
        state::{StaticRecipe, StaticState, compatibility, initialization::InitializationAttempt},
    },
};
use kagari_common::identity::table::DefinitionId;
use std::{
    collections::{HashMap, TryReserveError},
    ops::Index,
    sync::Arc,
};

/// Monotonic within one runtime; IDs are never reused after retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct CellId(u64);

#[derive(Debug)]
pub(in crate::module) struct StaticCell {
    previous: Option<CellId>,
    next: Option<CellId>,
    pub(in crate::module) mark: OwnerMark,
    /// Immutable type provenance, not an executable graph root.
    pub(in crate::module) owner: ExecutableModule,
    pub(in crate::module) slot: usize,
    pub(in crate::module) lineage: InstallationId,
    pub(in crate::module) state: StaticState,
    pub(in crate::module) attempt: Option<Arc<InitializationAttempt>>,
}

/// The original cell map owns its occupied chain and repairs the one active sweep
/// continuation on every removal. New cells prepend, outside a captured chain.
#[derive(Debug, Default)]
pub(in crate::module) struct StaticCells {
    cells: HashMap<CellId, StaticCell>,
    head: Option<CellId>,
    sweep: Option<(u64, Option<CellId>)>,
    generation: u64,
}

#[derive(Debug)]
pub(in crate::module) struct StaticSweep {
    generation: u64,
}

impl StaticCells {
    pub(in crate::module) fn get(&self, id: &CellId) -> Option<&StaticCell> {
        self.cells.get(id)
    }

    pub(in crate::module) fn get_mut(&mut self, id: &CellId) -> Option<&mut StaticCell> {
        self.cells.get_mut(id)
    }

    pub(in crate::module) fn len(&self) -> usize {
        self.cells.len()
    }

    #[cfg(test)]
    pub(in crate::module) fn iter(&self) -> impl Iterator<Item = (&CellId, &StaticCell)> {
        self.cells.iter()
    }

    #[cfg(test)]
    pub(in crate::module) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.cells.try_reserve(additional)
    }

    fn insert(&mut self, id: CellId, mut cell: StaticCell) {
        assert!(!self.cells.contains_key(&id), "non-reused static identity");
        cell.previous = None;
        cell.next = self.head;
        if let Some(head) = self.head {
            self.cells
                .get_mut(&head)
                .expect("occupied static head")
                .previous = Some(id);
        }
        self.cells.insert(id, cell);
        self.head = Some(id);
    }

    pub(in crate::module) fn remove(
        &mut self,
        id: &CellId,
        graph: &mut GraphMarker<'_>,
    ) -> Result<Option<StaticCell>, RuntimeError> {
        let Some(cell) = self.cells.get(id) else {
            return Ok(None);
        };
        graph.can_retire(cell.mark.as_ref())?;
        let cell = self.cells.remove(id).expect("checked static cell");
        if let Some(previous) = cell.previous {
            self.cells
                .get_mut(&previous)
                .expect("occupied static predecessor")
                .next = cell.next;
        } else {
            self.head = cell.next;
        }
        if let Some(next) = cell.next {
            self.cells
                .get_mut(&next)
                .expect("occupied static successor")
                .previous = cell.previous;
        }
        if let Some((_, next)) = &mut self.sweep
            && *next == Some(*id)
        {
            *next = cell.next;
        }
        graph.release_singleton(cell.mark.as_ref());
        graph.retire_prepared();
        Ok(Some(cell))
    }

    pub(in crate::module) fn begin_sweep(&mut self) -> Result<StaticSweep, RuntimeError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or_else(|| RuntimeError::resource_limit("static sweep generations"))?;
        self.sweep = Some((self.generation, self.head));
        Ok(StaticSweep {
            generation: self.generation,
        })
    }

    pub(in crate::module) fn sweep_next(
        &self,
        cursor: &StaticSweep,
    ) -> Result<Option<CellId>, RuntimeError> {
        self.sweep
            .filter(|(generation, _)| *generation == cursor.generation)
            .map(|(_, next)| next)
            .ok_or_else(|| RuntimeError::module_validation("stale static sweep cursor"))
    }

    pub(in crate::module) fn advance_sweep(
        &mut self,
        cursor: &StaticSweep,
        id: CellId,
    ) -> Result<(), RuntimeError> {
        if self.sweep_next(cursor)? != Some(id) {
            return Err(RuntimeError::module_validation(
                "static sweep cursor changed",
            ));
        }
        let next = self
            .cells
            .get(&id)
            .ok_or_else(|| RuntimeError::module_validation("missing occupied static cell"))?
            .next;
        self.sweep = Some((cursor.generation, next));
        Ok(())
    }
}

impl Index<&CellId> for StaticCells {
    type Output = StaticCell;

    fn index(&self, id: &CellId) -> &Self::Output {
        &self.cells[id]
    }
}

impl StaticCell {
    /// A detached frame can poison its attempt during termination/drop without
    /// borrowing storage. Completion is authoritative until the next transition.
    pub(in crate::module) fn snapshot(&self) -> StaticState {
        self.attempt
            .as_ref()
            .and_then(|attempt| attempt.failure())
            .map_or_else(
                || self.state.clone(),
                |failure| StaticState::Failed(failure.clone()),
            )
    }

    pub(in crate::module) fn failed_attempt(&self) -> bool {
        self.attempt
            .as_ref()
            .is_some_and(|attempt| attempt.failure().is_some())
    }
}

impl ModuleStoreInner {
    /// Validate only this generation's indexed binding; never search history or cells.
    pub(in crate::module) fn bound_cell(
        &self,
        record: &ModuleRecord,
        slot: usize,
    ) -> Option<&StaticCell> {
        if record.bindings.len() != record.module.bytecode.module_slots.len() {
            return None;
        }
        let expected = &record.module.bytecode.module_slots.get(slot)?.contract;
        let cell = self.cells.get(record.bindings.get(slot)?)?;
        if !self.graph.accessible(cell.mark.as_ref()).ok()? {
            return None;
        }
        let descriptor = cell.owner.bytecode.module_slots.get(cell.slot)?;
        if cell.lineage != record.static_lineage
            || cell.lineage != record.module.installation_id()
            || cell.owner.installation_id() != cell.lineage
            || &descriptor.contract != expected
        {
            return None;
        }
        if let StaticState::Uninitialized(recipe) | StaticState::Initializing(recipe) = &cell.state
            && (recipe.module.key() != cell.owner.key()
                || recipe.function != descriptor.initializer)
        {
            return None;
        }
        Some(cell)
    }

    pub(in crate::module) fn allocate_statics(
        &mut self,
        root: &ExecutableModule,
        graph: &GraphState,
    ) -> Result<(), RuntimeError> {
        let mut bindings = HashMap::new();
        let mut pending = Vec::new();
        let mut next = self.next_cell;
        for owner in root.members() {
            for (slot, descriptor) in owner.bytecode.module_slots.iter().enumerate() {
                if descriptor.contract.supplier != owner.bytecode.identity {
                    continue;
                }
                if !TypeView::new(&descriptor.contract.ty, &owner, None).is_heap_type() {
                    return Err(RuntimeError::module_validation(
                        "static storage requires a durable semantic type",
                    ));
                }
                next = next
                    .checked_add(1)
                    .ok_or_else(|| RuntimeError::resource_limit("static identity space"))?;
                let id = CellId(next);
                if bindings
                    .insert(descriptor.contract.declaration, id)
                    .is_some()
                {
                    return Err(RuntimeError::module_validation(
                        "duplicate static declaration",
                    ));
                }
                pending.push((
                    id,
                    StaticCell {
                        previous: None,
                        next: None,
                        mark: OwnerMark::default(),
                        owner: owner.clone(),
                        slot,
                        lineage: root.installation_id(),
                        attempt: None,
                        state: StaticState::Uninitialized(StaticRecipe {
                            module: owner.clone(),
                            function: descriptor.initializer,
                        }),
                    },
                ));
            }
        }
        let plans = root
            .members()
            .map(|owner| {
                let slots = owner
                    .bytecode
                    .module_slots
                    .iter()
                    .map(|descriptor| {
                        bindings
                            .get(&descriptor.contract.declaration)
                            .copied()
                            .ok_or_else(|| {
                                RuntimeError::module_validation("unlinked static supplier")
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok((owner.key(), slots))
            })
            .collect::<Result<Vec<_>, RuntimeError>>()?;
        self.next_cell = next;
        for (id, cell) in pending {
            let mut graph = graph.try_lock()?;
            graph.prepare_identity()?;
            self.cells
                .try_reserve(1)
                .map_err(|_| RuntimeError::resource_limit("static cell storage"))?;
            let scan = StaticScanId {
                owner: cell.owner.registry_owner,
                cell: id,
            };
            self.cells.insert(id, cell);
            graph.admit_prepared(GraphIdentity::Static(scan), self.cells[&id].mark.as_ref());
        }
        for (key, bindings) in plans {
            self.records.get_mut(&key).expect("staged member").bindings = bindings;
        }
        Ok(())
    }
}

impl ModuleStore {
    /// Build all replacement bindings before touching candidate records. Existing
    /// cells are only referenced: staging never executes or mutates their state.
    pub(crate) fn bind_static_reload(
        &self,
        baseline: &ExecutableModule,
        candidate: &ExecutableModule,
    ) -> Result<(), RuntimeError> {
        if baseline.registry_owner != candidate.registry_owner
            || baseline.installation_id() != candidate.installation_id()
        {
            return Err(RuntimeError::module_validation(
                "static reload requires the same installation",
            ));
        }
        let mut store = self
            .inner
            .try_borrow_mut()
            .map_err(|_| RuntimeError::module_validation("static store borrowed during staging"))?;
        let mut existing = HashMap::<DefinitionId, CellId>::new();
        for member in baseline.members() {
            let record = store
                .resolve(&member)
                .ok_or_else(|| RuntimeError::module_validation("reload baseline retired"))?;
            for (slot, descriptor) in member.bytecode.module_slots.iter().enumerate() {
                if descriptor.contract.supplier == member.bytecode.identity {
                    store.bound_cell(record, slot).ok_or_else(|| {
                        RuntimeError::module_validation("invalid baseline static binding")
                    })?;
                    existing.insert(descriptor.contract.declaration, record.bindings[slot]);
                }
            }
        }
        let mut remaining = existing.clone();
        let mut plans = Vec::new();
        for member in candidate.members() {
            let record = store
                .resolve(&member)
                .ok_or_else(|| RuntimeError::module_validation("reload candidate retired"))?;
            let mut bindings = record.bindings.clone();
            for (slot, descriptor) in member.bytecode.module_slots.iter().enumerate() {
                store.bound_cell(record, slot).ok_or_else(|| {
                    RuntimeError::module_validation("invalid candidate static binding")
                })?;
                if let Some(id) = existing.get(&descriptor.contract.declaration) {
                    let cell = &store.cells[id];
                    let previous = &cell.owner.bytecode.module_slots[cell.slot].contract;
                    if previous != &descriptor.contract
                        || !compatibility::compatible(&previous.ty, &cell.owner, &member)
                    {
                        return Err(RuntimeError::module_validation(
                            "incompatible persistent static storage contract",
                        ));
                    }
                    remaining.remove(&descriptor.contract.declaration);
                    bindings[slot] = *id;
                }
            }
            plans.push((member.key(), bindings));
        }
        if !remaining.is_empty() {
            return Err(RuntimeError::module_validation(
                "persistent static deletion, rename or move",
            ));
        }
        let mut graph = self.graph.try_lock()?;
        // Prepare every replacement before changing any binding. In particular,
        // sweep cannot revive a baseline cell merely because storage still exists.
        for (_, bindings) in &plans {
            for id in bindings {
                let cell = &store.cells[id];
                graph.shade(
                    GraphIdentity::Static(StaticScanId {
                        owner: cell.owner.registry_owner,
                        cell: *id,
                    }),
                    cell.mark.as_ref(),
                )?;
            }
        }
        // Both publications already belong to the same installation. Reuse cells
        // without changing their introducing publication or lazy initializer recipe.
        for (key, bindings) in plans {
            let record = store.records.get_mut(&key).expect("staged member");
            record.bindings = bindings;
        }
        // Superseded candidate-only cells are no longer graph roots. Leave them
        // to the bounded occupied-cell sweep, including gray cells with queued
        // work. Reload neither rebuilds a global binding set nor destroys records
        // under this borrow, and newborn cells stay outside an active sweep.
        Ok(())
    }
}
