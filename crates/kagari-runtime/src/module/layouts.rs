//! Concrete aggregate applications are cached within their loaded generation.
use crate::{
    frame::types::{TypeEnvironment, compatibility::TypeView},
    module::{EnumVariantRef, LoadedModule, StructLayoutRef},
};
use kagari_bytecode::instruction::{EnumId, StructId};
use kagari_common::identity::table::DefinitionId;
use kagari_contract::{
    layout::{EnumLayout, StructLayout},
    types::{NominalTy, Ty},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type LayoutApplications<Id, Layout> =
    RefCell<HashMap<Id, HashMap<Vec<Ty<DefinitionId>>, Rc<Layout>>>>;

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
            let mut caches = self.layouts.structures.borrow_mut();
            let cache = caches.entry(id).or_default();
            if let Some(layout) = cache.get(arguments) {
                Some(layout.clone())
            } else {
                let layout = Rc::new(template.apply(arguments, &Default::default())?.into_owned());
                cache.insert(arguments.to_vec(), layout.clone());
                Some(layout)
            }
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
            let mut caches = self.layouts.enumerations.borrow_mut();
            let cache = caches.entry(id).or_default();
            if let Some(layout) = cache.get(arguments) {
                Some(layout.clone())
            } else {
                let layout = Rc::new(template.apply(arguments, &Default::default())?.into_owned());
                cache.insert(arguments.to_vec(), layout.clone());
                Some(layout)
            }
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

impl StructLayoutRef {
    pub(crate) fn template(&self) -> &StructLayout<DefinitionId> {
        &self.module.bytecode.structures[self.id.index()]
    }

    pub(super) fn type_expression(&self) -> Ty<DefinitionId> {
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
        environment: Option<&TypeEnvironment>,
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
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeEnvironment>)> {
        let layout = if self.environment.is_some() {
            &self.module.bytecode.structures[self.id.index()]
        } else {
            self.layout()
        };
        Some((&layout.fields.get(slot)?.ty, self.environment.as_deref()))
    }
}

impl EnumVariantRef {
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
        environment: Option<&TypeEnvironment>,
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
        self.module.registry_owner == other.module.registry_owner
            && self.variant == other.variant
            && self.matches_type(
                &other.type_expression(),
                &other.module,
                other.environment.as_deref(),
            )
    }

    pub(crate) fn payload_type(
        &self,
        slot: usize,
    ) -> Option<(&Ty<DefinitionId>, Option<&TypeEnvironment>)> {
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
