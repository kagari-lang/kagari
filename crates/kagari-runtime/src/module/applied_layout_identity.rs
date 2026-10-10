//! Normalize complete applied layout meaning within one immutable program generation.
use crate::{
    frame::types::compatibility::{TypeIdentity, TypeView},
    module::{
        LoadedModule,
        descriptor_index::DescriptorIndex,
        layout_identity::{LayoutIdentity, ProgramLayouts},
        layout_scope::LayoutScope,
    },
};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::layout::{EnumLayout, StructLayout};
use kagari_types::ty::Ty;
use std::{hash::Hash, sync::Arc};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Meaning<L> {
    shape: Arc<L>,
    arguments: Arc<[Arc<TypeIdentity>]>,
}

#[derive(Debug)]
pub(super) struct IdentityIndex<L> {
    entries: DescriptorIndex<(), Meaning<L>, LayoutIdentity>,
    next: usize,
}

impl<L: Clone + Eq + Hash> IdentityIndex<L> {
    fn new(linked_count: usize) -> Self {
        Self {
            entries: Default::default(),
            next: linked_count,
        }
    }

    pub(super) fn prepare(
        &mut self,
        owner: &LoadedModule,
        shape: Arc<L>,
        arguments: &[Ty<DefinitionId>],
        scope: Option<&LayoutScope>,
        linked: Option<LayoutIdentity>,
    ) -> Option<LayoutIdentity> {
        let compiled = || {
            arguments
                .iter()
                .map(|ty| TypeView::new(ty, owner, None).identity().map(Arc::new))
                .collect::<Option<Arc<[_]>>>()
        };
        let arguments = match scope {
            Some(scope) => {
                // A scoped producer may have exactly the linked consumer's meaning.
                // Equality includes every nominal supplying generation, not names.
                if linked.is_some() && compiled().as_ref() == Some(scope.arguments()) {
                    return linked;
                }
                scope.arguments().clone()
            }
            None => {
                if linked.is_some() {
                    return linked;
                }
                compiled()?
            }
        };
        let key = Meaning { shape, arguments };
        if let Some(identity) = self.entries.get(&(), &key) {
            return Some(*identity);
        }
        // The counter outlives retained entries. Eviction never recycles an ID;
        // exhaustion simply retains the full compatibility path.
        let identity = LayoutIdentity::next(&mut self.next)?;
        let _ = self.entries.insert((), key, identity);
        Some(identity)
    }
}

#[derive(Debug)]
pub(super) struct AppliedIdentities {
    pub(super) structures: IdentityIndex<StructLayout<DefinitionId>>,
    pub(super) enumerations: IdentityIndex<EnumLayout<DefinitionId>>,
}

impl AppliedIdentities {
    pub(super) fn new(layouts: &ProgramLayouts) -> Self {
        let (structures, enumerations) = layouts.counts();
        Self {
            structures: IdentityIndex::new(structures),
            enumerations: IdentityIndex::new(enumerations),
        }
    }
}
