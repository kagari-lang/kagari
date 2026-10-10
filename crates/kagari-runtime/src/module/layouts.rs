//! Immutable aggregate descriptions; installed application caches live in ModuleStore.
use crate::{
    frame::types::{bindings::TypeBindings, compatibility::TypeView},
    module::{EnumVariantRef, LoadedModule, ModuleStore, StructLayoutRef},
};
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::layout::{EnumLayout, StructLayout};
use kagari_types::ty::{NominalTy, Ty};
use std::{collections::HashMap, sync::Arc};

type LayoutApplications<Id, Layout> = HashMap<Id, HashMap<Vec<Ty<DefinitionId>>, Arc<Layout>>>;

#[derive(Debug, Default)]
pub(super) struct LayoutCache {
    structures: LayoutApplications<StructId, StructLayout<DefinitionId>>,
    enumerations: LayoutApplications<EnumId, EnumLayout<DefinitionId>>,
}

impl LoadedModule {
    pub(crate) fn find_struct_layout(
        &self,
        nominal: &NominalTy<DefinitionId>,
    ) -> Option<StructLayoutRef> {
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
            owner.applied_struct_layout(StructId::new(id), &nominal.arguments)
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
            applied,
            environment: None,
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
            applied,
            environment: None,
        })
    }
}

impl ModuleStore {
    pub(crate) fn applied_struct_layout(
        &self,
        owner: &LoadedModule,
        id: StructId,
        arguments: &[Ty<DefinitionId>],
    ) -> Option<StructLayoutRef> {
        let mut records = self.inner.try_borrow_mut().ok();
        let Some(record) = records
            .as_deref_mut()
            .and_then(|records| records.resolve_mut(owner))
        else {
            // Detached type provenance remains readable without executable storage.
            return owner.applied_struct_layout(id, arguments);
        };
        if let Some(applied) = record
            .layouts
            .structures
            .get(&id)
            .and_then(|cache| cache.get(arguments))
        {
            return Some(StructLayoutRef {
                module: owner.clone(),
                id,
                applied: Some(applied.clone()),
                environment: None,
            });
        }
        let layout = owner.applied_struct_layout(id, arguments)?;
        if let Some(applied) = &layout.applied {
            record
                .layouts
                .structures
                .entry(id)
                .or_default()
                .insert(arguments.to_vec(), applied.clone());
        }
        Some(layout)
    }

    pub(crate) fn applied_enum_variant(
        &self,
        owner: &LoadedModule,
        id: EnumId,
        arguments: &[Ty<DefinitionId>],
        variant: u32,
    ) -> Option<EnumVariantRef> {
        let mut records = self.inner.try_borrow_mut().ok();
        let Some(record) = records
            .as_deref_mut()
            .and_then(|records| records.resolve_mut(owner))
        else {
            return owner.applied_enum_variant(id, arguments, variant);
        };
        if let Some(applied) = record
            .layouts
            .enumerations
            .get(&id)
            .and_then(|cache| cache.get(arguments))
        {
            applied.variants.get(variant as usize)?;
            return Some(EnumVariantRef {
                module: owner.clone(),
                id,
                variant,
                applied: Some(applied.clone()),
                environment: None,
            });
        }
        let layout = owner.applied_enum_variant(id, arguments, variant)?;
        if let Some(applied) = &layout.applied {
            record
                .layouts
                .enumerations
                .entry(id)
                .or_default()
                .insert(arguments.to_vec(), applied.clone());
        }
        Some(layout)
    }
}

impl StructLayoutRef {
    pub(crate) fn template(&self) -> &StructLayout<DefinitionId> {
        &self.module.bytecode.structures[self.id.index()]
    }

    pub(crate) fn type_expression(&self) -> Ty<DefinitionId> {
        Ty::Struct(NominalTy {
            declaration: self.layout().declaration,
            arguments: if self.environment.is_some() {
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
        TypeView::new(
            &self.type_expression(),
            &self.module,
            self.environment.as_deref(),
        )
        .compatible(TypeView::new(ty, owner, environment))
    }

    pub(crate) fn field_type(
        &self,
        slot: usize,
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeBindings>)> {
        let layout = if self.environment.is_some() {
            &self.module.bytecode.structures[self.id.index()]
        } else {
            self.layout()
        };
        Some((&layout.fields.get(slot)?.ty, self.environment.as_deref()))
    }
}

impl EnumVariantRef {
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
            arguments: if self.environment.is_some() {
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
        TypeView::new(
            &self.type_expression(),
            &self.module,
            self.environment.as_deref(),
        )
        .compatible(TypeView::new(ty, owner, environment))
    }

    /// Pattern access requires the concrete payload contract, not only a tag identity.
    pub fn matches_layout(&self, other: &Self) -> bool {
        if self.module.registry_owner != other.module.registry_owner
            || self.variant != other.variant
        {
            return false;
        }
        // Prepared descriptors identify the complete immutable payload scope.
        // Other applications and generations retain structural compatibility checks.
        if Arc::ptr_eq(&self.module.program, &other.module.program)
            && self.module.slot == other.module.slot
            && self.id == other.id
            && self.variant == other.variant
            && match (&self.applied, &other.applied) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
            && match (&self.environment, &other.environment) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
        {
            return true;
        }
        // Native results and their consuming patterns can use different module
        // slots/templates in the same pinned program. With no lexical bindings,
        // equal applied layouts resolve every nested nominal type through that
        // same immutable dependency graph. Reuse those layouts instead of
        // rebuilding both type applications during each compatibility check.
        if Arc::ptr_eq(&self.module.program, &other.module.program)
            && self.environment.is_none()
            && other.environment.is_none()
            && self.layout() == other.layout()
        {
            return true;
        }
        self.matches_type(
            &other.type_expression(),
            &other.module,
            other.environment.as_deref(),
        )
    }

    pub(crate) fn payload_type(
        &self,
        slot: usize,
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeBindings>)> {
        let layout = if self.environment.is_some() {
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
            self.environment.as_deref(),
        ))
    }
}
