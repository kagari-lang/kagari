//! Checked nominal contracts shared by local and imported member access.
mod implementations;
mod traits;
use crate::{
    declarations::{Declaration, DeclarationId, Declarations},
    hir::{Visibility, Writeability},
    imports::{ModuleGraph, SourceFunctionId},
    lower::LoweredModule,
    resolver::ResolvedName,
    typeck::{ModuleSignatures, TypedFunction},
    types::TypeId,
};
pub use implementations::{ImplementationSearchError, ImplementationSignature};
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
    pub visibility: Visibility,
    pub writeability: Writeability,
    pub ty: TypeId,
    pub declaration: Declaration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InherentMethodSignature {
    pub id: SourceFunctionId,
    pub declaration: DefinitionId,
    pub site: Declaration,
    pub owner: TypeId,
    pub visibility: Visibility,
    pub function: TypedFunction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructSignature {
    pub id: DefinitionId,
    pub generic_params: Vec<crate::types::GenericParameterType>,
    pub bounds: crate::typeck::GenericBounds,
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
    pub generic_params: Vec<crate::types::GenericParameterType>,
    pub bounds: crate::typeck::GenericBounds,
    pub declaration: Declaration,
    pub variants: Vec<VariantSignature>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AggregateCatalog {
    host_implementations: Vec<(crate::types::NominalType, TypeId)>,
    traits: BTreeMap<DefinitionId, Arc<TraitSignature>>,
    implementations: BTreeMap<DefinitionId, Arc<ImplementationSignature>>,
    methods: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    inherent_methods: BTreeMap<DefinitionId, Arc<InherentMethodSignature>>,
    structures: BTreeMap<DefinitionId, Arc<StructSignature>>,
    fields: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    enumerations: BTreeMap<DefinitionId, Arc<EnumSignature>>,
    variants: BTreeMap<DefinitionId, (DefinitionId, usize)>,
}

impl AggregateCatalog {
    pub fn inherent_methods(&self) -> impl Iterator<Item = &InherentMethodSignature> {
        self.inherent_methods.values().map(AsRef::as_ref)
    }
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
                    visibility: field.visibility,
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
                    generic_params: declarations.parameters_of(id),
                    bounds: signatures
                        .type_bounds(id)
                        .expect("checked type constraints")
                        .clone(),
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
                    generic_params: declarations.parameters_of(id),
                    bounds: signatures
                        .type_bounds(id)
                        .expect("checked type constraints")
                        .clone(),
                    declaration: declaration.clone(),
                    variants,
                }),
            );
        }
        self.add_traits(lowered, declarations, signatures, cancel)?;
        self.add_implementations(declarations, signatures, cancel)?;
        for host in declarations.hosts.type_declarations() {
            for implementation in &host.trait_implementations {
                cancel.check()?;
                if implementation.trait_id.module == *lowered.source.module_identity() {
                    self.host_implementations.push((
                        crate::host::HostDeclarations::trait_type(implementation),
                        TypeId::Host(host.id.clone()),
                    ));
                }
            }
        }
        for implementation in &lowered.module.impls {
            cancel.check()?;
            if implementation.trait_ref.is_some() {
                continue;
            }
            let Some(owner) = implementation
                .for_type
                .and_then(|ty| signatures.type_table().type_ref(ty))
                .map(|resolved| resolved.ty.clone())
            else {
                continue;
            };
            for method in &implementation.methods {
                cancel.check()?;
                let Some(function) = signatures
                    .functions()
                    .iter()
                    .find(|item| item.id == method.function)
                else {
                    continue;
                };
                let Some(declaration) =
                    declarations.definition(ResolvedName::Function(method.function))
                else {
                    continue;
                };
                let Some(lowered_function) = lowered
                    .module
                    .functions
                    .iter()
                    .find(|item| item.id == method.function)
                else {
                    continue;
                };
                let Some(site) = declarations.target(ResolvedName::Function(method.function))
                else {
                    continue;
                };
                self.inherent_methods.insert(
                    declaration.clone(),
                    Arc::new(InherentMethodSignature {
                        id: SourceFunctionId {
                            file: lowered.source.id(),
                            revision: lowered.source.revision(),
                            function: method.function,
                        },
                        declaration: declaration.clone(),
                        site: site.clone(),
                        owner: owner.clone(),
                        visibility: lowered_function.visibility,
                        function: function.clone(),
                    }),
                );
            }
        }
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
            for (id, implementation) in self
                .implementations
                .range(start.clone()..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                result
                    .implementations
                    .insert(id.clone(), implementation.clone());
            }
            for (id, method) in self
                .inherent_methods
                .range(start.clone()..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                result.inherent_methods.insert(id.clone(), method.clone());
            }
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
        result.host_implementations = self
            .host_implementations
            .iter()
            .filter(|(interface, _)| result.traits.contains_key(&interface.declaration))
            .cloned()
            .collect();
        Ok(result)
    }

    pub(crate) fn same_contracts(&self, other: &Self) -> bool {
        self.same_trait_contracts(other)
            && self.host_implementations == other.host_implementations
            && self.implementations == other.implementations
            && self.inherent_methods == other.inherent_methods
            && self.enumerations.len() == other.enumerations.len()
            && self.enumerations.iter().all(|(id, a)| {
                other.enumeration(id).is_some_and(|b| {
                    a.generic_params == b.generic_params
                        && a.bounds == b.bounds
                        && a.variants.len() == b.variants.len()
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
                    a.generic_params == b.generic_params
                        && a.bounds == b.bounds
                        && a.fields.len() == b.fields.len()
                        && a.fields.iter().zip(&b.fields).all(|(a, b)| {
                            a.id == b.id
                                && a.slot == b.slot
                                && a.name == b.name
                                && a.visibility == b.visibility
                                && a.writeability == b.writeability
                                && a.ty == b.ty
                        })
                })
            })
    }
}
