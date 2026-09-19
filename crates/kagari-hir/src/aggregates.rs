//! Checked nominal contracts shared by local and imported member access.
mod traits;
use crate::{
    declarations::{Declaration, DeclarationId, Declarations},
    hir::Writeability,
    imports::ModuleGraph,
    lower::LoweredModule,
    resolver::ResolvedName,
    typeck::ModuleSignatures,
    types::TypeId,
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{DefinitionId, ModuleIdentity},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub use traits::{MethodParameter, MethodSignature, TraitSignature};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSignature {
    pub id: DefinitionId,
    pub owner: DefinitionId,
    pub slot: usize,
    pub name: String,
    pub writeability: Writeability,
    pub ty: TypeId,
    pub declaration: Declaration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructSignature {
    pub id: DefinitionId,
    pub declaration: Declaration,
    pub fields: Vec<FieldSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSignature {
    pub id: DefinitionId,
    pub owner: DefinitionId,
    pub slot: usize,
    pub name: String,
    pub payload: Vec<TypeId>,
    pub declaration: Declaration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSignature {
    pub id: DefinitionId,
    pub declaration: Declaration,
    pub variants: Vec<VariantSignature>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AggregateCatalog {
    traits: BTreeMap<DefinitionId, Arc<TraitSignature>>,
    methods: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    structures: BTreeMap<DefinitionId, Arc<StructSignature>>,
    fields: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    enumerations: BTreeMap<DefinitionId, Arc<EnumSignature>>,
    variants: BTreeMap<DefinitionId, (DefinitionId, usize)>,
}

impl AggregateCatalog {
    pub fn enumerations(&self) -> impl Iterator<Item = &EnumSignature> {
        self.enumerations.values().map(AsRef::as_ref)
    }
    pub fn enumeration(&self, id: &DefinitionId) -> Option<&EnumSignature> {
        self.enumerations.get(id).map(AsRef::as_ref)
    }
    pub fn variant(&self, id: &DefinitionId) -> Option<&VariantSignature> {
        let (owner, slot) = self.variants.get(id)?;
        self.enumeration(owner)?.variants.get(*slot)
    }

    pub fn structures(&self) -> impl Iterator<Item = &StructSignature> {
        self.structures.values().map(AsRef::as_ref)
    }
    pub fn structure(&self, id: &DefinitionId) -> Option<&StructSignature> {
        self.structures.get(id).map(AsRef::as_ref)
    }
    pub fn field(&self, id: &DefinitionId) -> Option<&FieldSignature> {
        let (owner, slot) = self.fields.get(id)?;
        self.structure(owner)?.fields.get(*slot)
    }

    pub(crate) fn add_module(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        for structure in &lowered.module.structs {
            cancel.check()?;
            let Some(declaration) = declarations.target(ResolvedName::Struct(structure.id)) else {
                continue;
            };
            let DeclarationId::Definition(id) = &declaration.id else {
                continue;
            };
            let mut fields = Vec::new();
            for field in &structure.fields {
                cancel.check()?;
                let Some(declaration) = declarations.field(field.id) else {
                    continue;
                };
                let DeclarationId::Definition(field_id) = &declaration.id else {
                    continue;
                };
                self.fields
                    .insert(field_id.clone(), (id.clone(), fields.len()));
                fields.push(FieldSignature {
                    id: field_id.clone(),
                    owner: id.clone(),
                    slot: field.id.slot(),
                    name: field.name.clone(),
                    writeability: field.writeability,
                    ty: signatures
                        .type_table()
                        .field_type(field.id)
                        .unwrap_or(TypeId::Error),
                    declaration: declaration.clone(),
                });
            }
            self.structures.insert(
                id.clone(),
                Arc::new(StructSignature {
                    id: id.clone(),
                    declaration: declaration.clone(),
                    fields,
                }),
            );
        }
        for enumeration in &lowered.module.enums {
            cancel.check()?;
            let declaration = declarations
                .target(ResolvedName::Enum(enumeration.id))
                .expect("lowered enum has a declaration");
            let DeclarationId::Definition(id) = &declaration.id else {
                unreachable!("enum has nominal identity");
            };
            let mut variants = Vec::new();
            for variant in &enumeration.variants {
                cancel.check()?;
                let declaration = declarations
                    .variant(variant.id)
                    .expect("variant declaration");
                let DeclarationId::Definition(variant_id) = &declaration.id else {
                    unreachable!("variant has nominal identity");
                };
                let mut payload = Vec::new();
                for ty in &variant.payload {
                    cancel.check()?;
                    payload.push(
                        signatures
                            .type_table()
                            .type_ref(*ty)
                            .expect("signature query visits every payload type")
                            .ty
                            .clone(),
                    );
                }
                self.variants
                    .insert(variant_id.clone(), (id.clone(), variants.len()));
                variants.push(VariantSignature {
                    id: variant_id.clone(),
                    owner: id.clone(),
                    slot: variant.id.slot(),
                    name: variant.name.clone(),
                    payload,
                    declaration: declaration.clone(),
                });
            }
            self.enumerations.insert(
                id.clone(),
                Arc::new(EnumSignature {
                    id: id.clone(),
                    declaration: declaration.clone(),
                    variants,
                }),
            );
        }
        self.add_traits(lowered, declarations, signatures, cancel)?;
        Ok(())
    }

    pub(crate) fn for_module(
        &self,
        root: &ModuleIdentity,
        graph: &ModuleGraph,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut reachable = BTreeSet::new();
        let mut pending = vec![root.clone()];
        while let Some(module) = pending.pop() {
            cancel.check()?;
            if !reachable.insert(module.clone()) {
                continue;
            }
            if let Some(node) = graph.node(&module) {
                pending.extend(node.dependencies().iter().cloned());
            }
        }
        let mut result = Self::default();
        for module in reachable {
            cancel.check()?;
            self.include_traits(&mut result, &module, cancel)?;
            let start = DefinitionId {
                module: module.clone(),
                path: Vec::new(),
            };
            for (id, structure) in self
                .structures
                .range(start.clone()..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                for (index, field) in structure.fields.iter().enumerate() {
                    cancel.check()?;
                    result.fields.insert(field.id.clone(), (id.clone(), index));
                }
                result.structures.insert(id.clone(), structure.clone());
            }
            for (id, enumeration) in self
                .enumerations
                .range(start..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                for (index, variant) in enumeration.variants.iter().enumerate() {
                    cancel.check()?;
                    result
                        .variants
                        .insert(variant.id.clone(), (id.clone(), index));
                }
                result.enumerations.insert(id.clone(), enumeration.clone());
            }
        }
        Ok(result)
    }

    pub(crate) fn same_contracts(&self, other: &Self) -> bool {
        self.same_trait_contracts(other)
            && self.enumerations.len() == other.enumerations.len()
            && self.enumerations.iter().all(|(id, a)| {
                other.enumeration(id).is_some_and(|b| {
                    a.variants.len() == b.variants.len()
                        && a.variants.iter().zip(&b.variants).all(|(a, b)| {
                            a.id == b.id
                                && a.slot == b.slot
                                && a.name == b.name
                                && a.payload == b.payload
                        })
                })
            })
            && self.structures.len() == other.structures.len()
            && self.structures.iter().all(|(id, a)| {
                other.structure(id).is_some_and(|b| {
                    a.fields.len() == b.fields.len()
                        && a.fields.iter().zip(&b.fields).all(|(a, b)| {
                            a.id == b.id
                                && a.slot == b.slot
                                && a.name == b.name
                                && a.writeability == b.writeability
                                && a.ty == b.ty
                        })
                })
            })
    }
}
