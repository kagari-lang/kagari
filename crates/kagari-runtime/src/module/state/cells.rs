//! Allocation and transactional generation bindings. The module store owns cells;
//! immutable loaded descriptors and prepared programs never own mutable state.
use crate::{
    error::RuntimeError,
    frame::types::compatibility::TypeView,
    module::{
        LoadedModule, ModuleKey, ModuleStore, ModuleStoreInner,
        records::ModuleRecord,
        state::{StaticRecipe, StaticState, compatibility, initialization::InitializationAttempt},
    },
};
use kagari_common::identity::table::DefinitionId;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Monotonic within one runtime; IDs are never reused after retirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::module) struct CellId(u64);

#[derive(Debug)]
pub(in crate::module) struct StaticCell {
    /// Immutable type provenance, not an executable graph root.
    pub(in crate::module) owner: LoadedModule,
    pub(in crate::module) slot: usize,
    pub(in crate::module) lineage: ModuleKey,
    pub(in crate::module) state: StaticState,
    pub(in crate::module) attempt: Option<Arc<InitializationAttempt>>,
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
        let descriptor = cell.owner.bytecode.module_slots.get(cell.slot)?;
        if cell.lineage != record.static_lineage || &descriptor.contract != expected {
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
        root: &LoadedModule,
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
                        owner: owner.clone(),
                        slot,
                        lineage: root.program_key(),
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
        self.cells.extend(pending);
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
        baseline: &LoadedModule,
        candidate: &LoadedModule,
    ) -> Result<(), RuntimeError> {
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
        // The introducing program key is the private installation lineage. A new
        // ordinary load gets fresh cells even when the installation name is reused.
        let lineage = store
            .resolve(baseline)
            .expect("checked baseline")
            .static_lineage;
        for (key, bindings) in plans {
            for id in &bindings {
                if let Some(cell) = store.cells.get_mut(id)
                    && cell.owner.program_key() == candidate.program_key()
                {
                    cell.lineage = lineage;
                }
            }
            let record = store.records.get_mut(&key).expect("staged member");
            record.bindings = bindings;
            record.static_lineage = lineage;
        }
        // Superseded candidate-only allocations have no observable owners.
        let referenced = store
            .records
            .values()
            .flat_map(|record| record.bindings.iter().copied())
            .collect::<HashSet<_>>();
        store.cells.retain(|id, _| referenced.contains(id));
        Ok(())
    }
}
