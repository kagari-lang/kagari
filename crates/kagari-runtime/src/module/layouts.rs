//! Immutable aggregate descriptions; installed application caches live in ModuleStore.
use crate::{
    frame::types::{bindings::TypeBindings, compatibility::TypeView},
    module::{
        EnumVariantRef, LoadedModule, ModuleStore, StructLayoutRef,
        applied_layout_identity::AppliedIdentities,
        descriptor_index::DescriptorIndex,
        layout_admission::{self, AggregateKind, LayoutEndpoint},
        layout_identity::{LayoutIdentity, ProgramLayouts},
        layout_scope::{LayoutScope, LayoutScopes},
    },
};
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::layout::{EnumLayout, StructLayout};
use kagari_types::ty::{NominalTy, Ty};
use std::{num::NonZeroUsize, sync::Arc};

type LayoutApplications<Id, Layout> =
    DescriptorIndex<(Id, Option<NonZeroUsize>), Arc<[Ty<DefinitionId>]>, AppliedLayout<Layout>>;

#[derive(Debug)]
struct AppliedLayout<L> {
    layout: Option<Arc<L>>,
    canonical: Option<LayoutIdentity>,
    scope: Option<Arc<LayoutScope>>,
}

#[derive(Debug, Default)]
pub(super) struct LayoutCache {
    structures: LayoutApplications<StructId, StructLayout<DefinitionId>>,
    enumerations: LayoutApplications<EnumId, EnumLayout<DefinitionId>>,
}

/// Created lazily on the program-root record; members own only their applications.
#[derive(Debug)]
pub(super) struct ProgramLayoutCache {
    pub(super) scopes: LayoutScopes,
    identities: AppliedIdentities,
}

impl ProgramLayoutCache {
    pub(super) fn new(layouts: &ProgramLayouts) -> Self {
        Self {
            scopes: Default::default(),
            identities: AppliedIdentities::new(layouts),
        }
    }
}

impl LoadedModule {
    pub(crate) fn find_struct_layout(
        &self,
        nominal: &NominalTy<DefinitionId>,
    ) -> Option<StructLayoutRef> {
        let (owner, id) = self.find_struct_definition(nominal)?;
        owner.applied_struct_layout(id, &nominal.arguments)
    }

    pub(crate) fn find_struct_definition(
        &self,
        nominal: &NominalTy<DefinitionId>,
    ) -> Option<(LoadedModule, StructId)> {
        self.members().find_map(|owner| {
            let id = owner
                .bytecode
                .structures
                .iter()
                .enumerate()
                .filter(|(_, layout)| {
                    layout.declaration == nominal.declaration && layout.accepts(&nominal.arguments)
                })
                .max_by_key(|(_, layout)| !layout.arguments.iter().all(Ty::is_concrete))?
                .0;
            Some((owner, StructId::new(id)))
        })
    }

    pub(crate) fn find_enum_definition(
        &self,
        nominal: &NominalTy<DefinitionId>,
    ) -> Option<(LoadedModule, EnumId)> {
        self.members().find_map(|owner| {
            let id = owner
                .bytecode
                .enumerations
                .iter()
                .enumerate()
                .filter(|(_, layout)| {
                    layout.declaration == nominal.declaration && layout.accepts(&nominal.arguments)
                })
                .max_by_key(|(_, layout)| !layout.arguments.iter().all(Ty::is_concrete))?
                .0;
            Some((owner, EnumId::new(id)))
        })
    }

    pub fn applied_struct_layout(
        &self,
        id: StructId,
        arguments: &[Ty<DefinitionId>],
    ) -> Option<StructLayoutRef> {
        if !arguments.iter().all(Ty::is_concrete) {
            return None;
        }
        let template = self.bytecode.structures.get(id.index())?;
        if !template.accepts(arguments) {
            return None;
        }
        let applied = if template.arguments == arguments {
            None
        } else {
            Some(Arc::new(
                template.apply(arguments, &Default::default())?.into_owned(),
            ))
        };
        Some(StructLayoutRef {
            module: self.clone(),
            id,
            canonical: match &applied {
                None => Some(self.program.layouts.structure(self.slot, id)),
                Some(layout) => self
                    .program
                    .layouts
                    .applied_structure(layout, &self.program.code.modules),
            },
            applied,
            scope: None,
        })
    }

    pub fn applied_enum_variant(
        &self,
        id: EnumId,
        arguments: &[Ty<DefinitionId>],
        variant: u32,
    ) -> Option<EnumVariantRef> {
        if !arguments.iter().all(Ty::is_concrete) {
            return None;
        }
        let template = self.bytecode.enumerations.get(id.index())?;
        if !template.accepts(arguments) {
            return None;
        }
        template.variants.get(variant as usize)?;
        let applied = if template.arguments == arguments {
            None
        } else {
            Some(Arc::new(
                template.apply(arguments, &Default::default())?.into_owned(),
            ))
        };
        Some(EnumVariantRef {
            module: self.clone(),
            id,
            variant,
            canonical: match &applied {
                None => Some(self.program.layouts.enumeration(self.slot, id)),
                Some(layout) => self
                    .program
                    .layouts
                    .applied_enumeration(layout, &self.program.code.modules),
            },
            applied,
            scope: None,
        })
    }
}

impl ModuleStore {
    pub(crate) fn applied_struct_layout(
        &self,
        owner: &LoadedModule,
        id: StructId,
        arguments: &[Ty<DefinitionId>],
        scope: Option<Arc<LayoutScope>>,
    ) -> Option<StructLayoutRef> {
        let cache_key = match &scope {
            Some(scope) => scope.id_for(owner).map(|scope| (id, Some(scope))),
            None => Some((id, None)),
        };
        let mut records = self.inner.try_borrow_mut().ok();
        if let Some(applied) = records
            .as_deref()
            .and_then(|records| records.resolve(owner))
            .and_then(|record| record.layouts.structures.get(&cache_key?, arguments))
        {
            return Some(StructLayoutRef {
                module: owner.clone(),
                id,
                applied: applied.layout.clone(),
                canonical: applied.canonical,
                scope: applied.scope.clone(),
            });
        }
        let mut layout = owner.applied_struct_layout(id, arguments)?;
        if scope
            .as_ref()
            .is_some_and(|scope| !scope.accepts(layout.layout().declaration, arguments))
        {
            return None;
        }
        layout.scope = scope;
        if layout.scope.is_some() || layout.canonical.is_none() {
            let linked = layout.canonical;
            // A canonical identity always describes both physical shape and scope.
            // Detached/borrowed-store preparation keeps full checks as its fallback.
            layout.canonical = records
                .as_deref_mut()
                .and_then(|records| records.resolve_mut(&owner.program_root()))
                .and_then(|record| record.program_layouts())
                .and_then(|layouts| {
                    layouts.identities.structures.prepare(
                        owner,
                        layout
                            .applied
                            .clone()
                            .unwrap_or_else(|| Arc::new(layout.layout().clone())),
                        arguments,
                        layout.scope.as_deref(),
                        linked,
                    )
                });
        }
        if (layout.applied.is_some() || layout.scope.is_some())
            && let Some((record, key)) = records
                .as_deref_mut()
                .and_then(|records| records.resolve_mut(owner))
                .zip(cache_key)
        {
            // Optional retention cannot invalidate a complete immutable descriptor.
            let _ = record.layouts.structures.insert(
                key,
                Arc::from(arguments),
                AppliedLayout {
                    layout: layout.applied.clone(),
                    canonical: layout.canonical,
                    scope: layout.scope.clone(),
                },
            );
        }
        Some(layout)
    }

    pub(crate) fn applied_enum_variant(
        &self,
        owner: &LoadedModule,
        id: EnumId,
        arguments: &[Ty<DefinitionId>],
        variant: u32,
        scope: Option<Arc<LayoutScope>>,
    ) -> Option<EnumVariantRef> {
        let cache_key = match &scope {
            Some(scope) => scope.id_for(owner).map(|scope| (id, Some(scope))),
            None => Some((id, None)),
        };
        let mut records = self.inner.try_borrow_mut().ok();
        if let Some(applied) = records
            .as_deref()
            .and_then(|records| records.resolve(owner))
            .and_then(|record| record.layouts.enumerations.get(&cache_key?, arguments))
        {
            applied
                .layout
                .as_deref()
                .unwrap_or(&owner.bytecode.enumerations[id.index()])
                .variants
                .get(variant as usize)?;
            return Some(EnumVariantRef {
                module: owner.clone(),
                id,
                variant,
                applied: applied.layout.clone(),
                canonical: applied.canonical,
                scope: applied.scope.clone(),
            });
        }
        let mut layout = owner.applied_enum_variant(id, arguments, variant)?;
        if scope
            .as_ref()
            .is_some_and(|scope| !scope.accepts(layout.layout().declaration, arguments))
        {
            return None;
        }
        layout.scope = scope;
        if layout.scope.is_some() || layout.canonical.is_none() {
            let linked = layout.canonical;
            // A canonical identity always describes both physical shape and scope.
            // Detached/borrowed-store preparation keeps full checks as its fallback.
            layout.canonical = records
                .as_deref_mut()
                .and_then(|records| records.resolve_mut(&owner.program_root()))
                .and_then(|record| record.program_layouts())
                .and_then(|layouts| {
                    layouts.identities.enumerations.prepare(
                        owner,
                        layout
                            .applied
                            .clone()
                            .unwrap_or_else(|| Arc::new(layout.layout().clone())),
                        arguments,
                        layout.scope.as_deref(),
                        linked,
                    )
                });
        }
        if (layout.applied.is_some() || layout.scope.is_some())
            && let Some((record, key)) = records
                .as_deref_mut()
                .and_then(|records| records.resolve_mut(owner))
                .zip(cache_key)
        {
            // Optional retention cannot invalidate a complete immutable descriptor.
            let _ = record.layouts.enumerations.insert(
                key,
                Arc::from(arguments),
                AppliedLayout {
                    layout: layout.applied.clone(),
                    canonical: layout.canonical,
                    scope: layout.scope.clone(),
                },
            );
        }
        Some(layout)
    }
}

impl StructLayoutRef {
    pub(crate) fn type_bindings(&self) -> Option<&Arc<TypeBindings>> {
        self.scope.as_ref().map(|scope| scope.bindings())
    }

    pub(crate) fn template(&self) -> &StructLayout<DefinitionId> {
        &self.module.bytecode.structures[self.id.index()]
    }

    pub(crate) fn type_expression(&self) -> Ty<DefinitionId> {
        Ty::Struct(NominalTy {
            declaration: self.layout().declaration,
            arguments: if self.scope.is_some() {
                self.template().arguments.clone()
            } else {
                self.layout().arguments.clone()
            },
            associated_types: Default::default(),
        })
    }

    pub(crate) fn matches_type(
        &self,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        self.matches_view(TypeView::new(ty, owner, environment))
    }

    pub(crate) fn matches_view(&self, expected: TypeView<'_>) -> bool {
        TypeView::new(
            &self.type_expression(),
            &self.module,
            self.type_bindings().map(Arc::as_ref),
        )
        .compatible(expected)
    }

    pub(crate) fn field_type(
        &self,
        slot: usize,
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeBindings>)> {
        let layout = if self.scope.is_some() {
            &self.module.bytecode.structures[self.id.index()]
        } else {
            self.layout()
        };
        Some((
            &layout.fields.get(slot)?.ty,
            self.type_bindings().map(Arc::as_ref),
        ))
    }
}

impl EnumVariantRef {
    pub(crate) fn type_bindings(&self) -> Option<&Arc<TypeBindings>> {
        self.scope.as_ref().map(|scope| scope.bindings())
    }

    /// Reuse prepared layout and payload scope for another checked member.
    pub(crate) fn with_variant(&self, variant: u32) -> Option<Self> {
        self.layout().variants.get(variant as usize)?;
        Some(Self {
            variant,
            ..self.clone()
        })
    }

    fn type_expression(&self) -> Ty<DefinitionId> {
        Ty::Enum(NominalTy {
            declaration: self.layout().declaration,
            arguments: if self.scope.is_some() {
                self.module.bytecode.enumerations[self.id.index()]
                    .arguments
                    .clone()
            } else {
                self.layout().arguments.clone()
            },
            associated_types: Default::default(),
        })
    }

    pub(crate) fn matches_type(
        &self,
        ty: &Ty<DefinitionId>,
        owner: &LoadedModule,
        environment: Option<&TypeBindings>,
    ) -> bool {
        self.matches_view(TypeView::new(ty, owner, environment))
    }

    pub(crate) fn matches_view(&self, expected: TypeView<'_>) -> bool {
        TypeView::new(
            &self.type_expression(),
            &self.module,
            self.type_bindings().map(Arc::as_ref),
        )
        .compatible(expected)
    }

    /// Pattern access requires the concrete payload contract, not only a tag identity.
    pub fn matches_layout(&self, other: &Self) -> bool {
        if self.module.registry_owner != other.module.registry_owner
            || self.variant != other.variant
        {
            return false;
        }
        // Complete layout identity and genuine cross-scope compatibility share
        // one admission owner, including aliases in different portable members.
        layout_admission::admit(
            AggregateKind::Enum,
            LayoutEndpoint {
                owner: &self.module,
                identity: self.canonical,
            },
            LayoutEndpoint {
                owner: &other.module,
                identity: other.canonical,
            },
            || {
                self.matches_type(
                    &other.type_expression(),
                    &other.module,
                    other.type_bindings().map(Arc::as_ref),
                )
            },
        )
    }

    pub(crate) fn payload_type(
        &self,
        slot: usize,
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeBindings>)> {
        let layout = if self.scope.is_some() {
            &self.module.bytecode.enumerations[self.id.index()]
        } else {
            self.layout()
        };
        Some((
            layout
                .variants
                .get(self.variant as usize)?
                .payload
                .get(slot)?,
            self.type_bindings().map(Arc::as_ref),
        ))
    }
}
