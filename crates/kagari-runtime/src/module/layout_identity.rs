//! Canonical layout locations in one immutable, linked program.
//! Hashes select candidates only; complete equality establishes identity at linking
//! or application preparation. The table borrows no runtime/GC resources.
use kagari_bytecode::{
    instruction::{EnumId, StructId},
    module::BytecodeModule,
    program::ModuleRef,
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::layout::{EnumLayout, StructLayout};
use std::{
    collections::{HashMap, HashSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    num::NonZeroUsize,
    sync::Arc,
};

/// Meaningful only within the same program descriptor and aggregate kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct LayoutIdentity {
    slot: NonZeroUsize,
}

impl LayoutIdentity {
    pub(super) fn next(counter: &mut usize) -> Option<Self> {
        *counter = counter.checked_add(1)?;
        Some(Self {
            slot: NonZeroUsize::new(*counter)?,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct LayoutLocation {
    member: ModuleRef,
    index: usize,
}

#[derive(Debug, Clone)]
struct LayoutTable {
    locations: Vec<LayoutLocation>,
    members: Vec<Vec<LayoutIdentity>>,
    candidates: HashMap<u64, Vec<LayoutIdentity>>,
}

impl LayoutTable {
    fn prepare<L: Eq + Hash>(modules: &[&[L]]) -> Self {
        let mut table = Self {
            locations: Vec::new(),
            members: Vec::with_capacity(modules.len()),
            candidates: HashMap::new(),
        };
        for (member, layouts) in modules.iter().enumerate() {
            let mut identities = Vec::with_capacity(layouts.len());
            for (index, layout) in layouts.iter().enumerate() {
                let candidates = table.candidates.entry(fingerprint(layout)).or_default();
                let identity = candidates
                    .iter()
                    .copied()
                    .find(|id| {
                        let location = table.locations[id.slot.get() - 1];
                        &modules[location.member.index()][location.index] == layout
                    })
                    .unwrap_or_else(|| {
                        let id = LayoutIdentity {
                            slot: NonZeroUsize::new(table.locations.len() + 1)
                                .expect("nonzero layout identity"),
                        };
                        table.locations.push(LayoutLocation {
                            member: ModuleRef::new(member),
                            index,
                        });
                        candidates.push(id);
                        id
                    });
                identities.push(identity);
            }
            table.members.push(identities);
        }
        table
    }

    fn applied<'a, L: Eq + Hash + 'a>(
        &self,
        layout: &L,
        get: impl Fn(LayoutLocation) -> &'a L,
    ) -> Option<LayoutIdentity> {
        self.candidates
            .get(&fingerprint(layout))?
            .iter()
            .copied()
            .find(|id| get(self.locations[id.slot.get() - 1]) == layout)
    }
}

/// Pure layout correspondence is shared by every installation of this product.
#[derive(Debug)]
pub(super) struct LayoutFacts {
    structures: LayoutTable,
    enumerations: LayoutTable,
}

impl LayoutFacts {
    pub(super) fn prepare(modules: &[Arc<BytecodeModule>]) -> Self {
        Self {
            structures: LayoutTable::prepare(
                &modules
                    .iter()
                    .map(|module| module.structures.as_slice())
                    .collect::<Vec<_>>(),
            ),
            enumerations: LayoutTable::prepare(
                &modules
                    .iter()
                    .map(|module| module.enumerations.as_slice())
                    .collect::<Vec<_>>(),
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct ProgramLayouts {
    facts: Arc<LayoutFacts>,
    // Provider authority is installation-local, never inferred from product names.
    shared_traits: HashSet<DefinitionId>,
}

impl ProgramLayouts {
    pub(super) fn prepare(facts: Arc<LayoutFacts>, shared_traits: HashSet<DefinitionId>) -> Self {
        Self {
            facts,
            shared_traits,
        }
    }

    pub(super) fn is_shared_native_trait(&self, declaration: DefinitionId) -> bool {
        self.shared_traits.contains(&declaration)
    }

    pub(super) fn counts(&self) -> (usize, usize) {
        (
            self.facts.structures.locations.len(),
            self.facts.enumerations.locations.len(),
        )
    }

    pub(super) fn structure(&self, member: ModuleRef, id: StructId) -> LayoutIdentity {
        self.facts.structures.members[member.index()][id.index()]
    }

    pub(super) fn enumeration(&self, member: ModuleRef, id: EnumId) -> LayoutIdentity {
        self.facts.enumerations.members[member.index()][id.index()]
    }

    pub(super) fn applied_structure(
        &self,
        layout: &StructLayout,
        modules: &[Arc<BytecodeModule>],
    ) -> Option<LayoutIdentity> {
        self.facts.structures.applied(layout, |id| {
            &modules[id.member.index()].structures[id.index]
        })
    }

    pub(super) fn applied_enumeration(
        &self,
        layout: &EnumLayout,
        modules: &[Arc<BytecodeModule>],
    ) -> Option<LayoutIdentity> {
        self.facts.enumerations.applied(layout, |id| {
            &modules[id.member.index()].enumerations[id.index]
        })
    }
}

fn fingerprint(layout: &impl Hash) -> u64 {
    let mut hash = DefaultHasher::new();
    layout.hash(&mut hash);
    hash.finish()
}
