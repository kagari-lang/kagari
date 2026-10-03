//! Checked nominal contracts shared by local and imported member access.

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{self, DefinitionPath, ModuleIdentity, reference::DefinitionReference},
};

use crate::{
    aggregates::{implementations::ImplementationSignature, traits::TraitSignature},
    declarations::{Declaration, DeclarationId, Declarations},
    hir::{item::storage::Visibility, writeability::Writeability},
    host::HostDeclarations,
    imports::{ModuleGraph, functions::SourceFunctionId},
    language::semantics as builtin_traits,
    lower::LoweredModule,
    native::NativeTypeKind,
    resolver::resolved::ResolvedName,
    typeck::{GenericBounds, ModuleSignatures, TypedFunction},
    types::{GenericParameterType, NominalType, TypeId},
};
pub mod implementations;
mod interfaces;
mod native;
pub mod protocols;
mod storage;
pub mod traits;

use kagari_contract::language::{Protocol, role::LangRole};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSignature<I: DefinitionReference = DefinitionPath> {
    pub id: I,
    pub owner: I,
    pub slot: usize,
    pub name: String,
    pub visibility: Visibility,
    pub writeability: Writeability,
    pub ty: TypeId<I>,
    pub declaration: Declaration<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InherentMethodSignature<I: DefinitionReference = DefinitionPath> {
    pub id: SourceFunctionId,
    pub declaration: I,
    pub site: Declaration<I>,
    pub owner: TypeId<I>,
    pub visibility: Visibility,
    pub function: TypedFunction<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructSignature<I: DefinitionReference = DefinitionPath> {
    pub id: I,
    pub generic_params: Vec<GenericParameterType<I>>,
    pub bounds: GenericBounds<I>,
    pub declaration: Declaration<I>,
    pub fields: Vec<FieldSignature<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTypeSignature<I: DefinitionReference = DefinitionPath> {
    pub id: I,
    pub generic_params: Vec<GenericParameterType<I>>,
    pub bounds: GenericBounds<I>,
    pub declaration: Declaration<I>,
    pub representation: NativeTypeKind<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSignature<I: DefinitionReference = DefinitionPath> {
    pub id: I,
    pub owner: I,
    pub slot: usize,
    pub name: String,
    pub payload: Vec<TypeId<I>>,
    pub declaration: Declaration<I>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSignature<I: DefinitionReference = DefinitionPath> {
    pub id: I,
    pub native_type: Option<NativeTypeKind<I>>,
    pub generic_params: Vec<GenericParameterType<I>>,
    pub bounds: GenericBounds<I>,
    pub declaration: Declaration<I>,
    pub variants: Vec<VariantSignature<I>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AggregateCatalog<I: DefinitionReference = DefinitionPath> {
    language_items: BTreeMap<LangRole, I>,
    implementation_constants: BTreeMap<I, BTreeMap<I, I>>,
    host_implementations: Vec<(NominalType<I>, TypeId<I>)>,
    traits: BTreeMap<I, Arc<TraitSignature<I>>>,
    implementations: BTreeMap<I, Arc<ImplementationSignature<I>>>,
    methods: BTreeMap<I, (I, usize)>,
    inherent_methods: BTreeMap<I, Arc<InherentMethodSignature<I>>>,
    native_types: BTreeMap<I, Arc<NativeTypeSignature<I>>>,
    structures: BTreeMap<I, Arc<StructSignature<I>>>,
    fields: BTreeMap<I, (I, usize)>,
    enumerations: BTreeMap<I, Arc<EnumSignature<I>>>,
    variants: BTreeMap<I, (I, usize)>,
}

impl AggregateCatalog {
    pub fn language_trait(&self, protocol: Protocol) -> Option<NominalType> {
        let role = LangRole::from_protocol(protocol)?;
        let declaration = self.language_items.get(&role)?.clone();
        self.trait_(&declaration)?;
        Some(NominalType {
            declaration,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        })
    }

    pub fn intrinsic_implementation(
        &self,
        interface: &NominalType,
        ty: &TypeId,
        bounds: &GenericBounds,
    ) -> bool {
        builtin_traits::intrinsic_applies(interface, ty, Some(self), bounds)
    }

    pub(crate) fn add_module(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        self.language_items.extend(
            declarations
                .language_items
                .iter()
                .map(|(role, id)| (*role, id.clone())),
        );
        for item in &lowered.module.opaque_types {
            cancel.check()?;
            let Some(declaration) = declarations.target(ResolvedName::OpaqueType(item.id)) else {
                continue;
            };
            let DeclarationId::Definition(id) = &declaration.id else {
                continue;
            };
            let Some(representation) = declarations.native_type(item.id) else {
                continue;
            };
            self.native_types.insert(
                id.clone(),
                Arc::new(NativeTypeSignature {
                    id: id.clone(),
                    generic_params: declarations.parameters_of(id),
                    bounds: signatures
                        .type_bounds(id)
                        .expect("checked native type constraints")
                        .clone(),
                    declaration: declaration.clone(),
                    representation,
                }),
            );
        }
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
                    native_type: declarations.native_enum(enumeration.id),
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
        self.add_implementations(lowered, declarations, signatures, cancel)?;
        for implementation in &lowered.module.impls {
            let Some(owner) = declarations.impl_identity(implementation.id) else {
                continue;
            };
            let Some(contract) = self.implementation_signature(owner) else {
                continue;
            };
            let constants = implementation
                .associated_consts
                .iter()
                .filter_map(|member| {
                    let initializer = member.initializer?;
                    Some((
                        identity::associated_const_id(
                            &contract.trait_type.declaration,
                            &member.name,
                        ),
                        declarations
                            .definition(ResolvedName::Const(initializer))?
                            .clone(),
                    ))
                })
                .collect();
            self.implementation_constants
                .insert(owner.clone(), constants);
        }
        for host in declarations.hosts.type_declarations() {
            for implementation in &host.trait_implementations {
                cancel.check()?;
                if implementation.trait_id.module == *lowered.source.module_identity() {
                    self.host_implementations.push((
                        HostDeclarations::trait_type(implementation),
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
        let mut result = Self {
            language_items: self.language_items.clone(),
            ..Self::default()
        };
        for module in reachable {
            cancel.check()?;
            self.include_traits(&mut result, &module, cancel)?;
            let start = DefinitionPath {
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
                if let Some(constants) = self.implementation_constants.get(id) {
                    result
                        .implementation_constants
                        .insert(id.clone(), constants.clone());
                }
            }
            for (id, method) in self
                .inherent_methods
                .range(start.clone()..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                result.inherent_methods.insert(id.clone(), method.clone());
            }
            for (id, native_type) in self
                .native_types
                .range(start.clone()..)
                .take_while(|(id, _)| id.module == module)
            {
                cancel.check()?;
                result.native_types.insert(id.clone(), native_type.clone());
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
            && self.implementation_constants == other.implementation_constants
            && self.host_implementations == other.host_implementations
            && self.implementations == other.implementations
            && self.inherent_methods == other.inherent_methods
            && self.enumerations.len() == other.enumerations.len()
            && self.enumerations.iter().all(|(id, a)| {
                other.enumeration(id).is_some_and(|b| {
                    a.native_type == b.native_type
                        && a.generic_params == b.generic_params
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

impl<I: DefinitionReference> Default for AggregateCatalog<I> {
    fn default() -> Self {
        Self {
            language_items: Default::default(),
            implementation_constants: Default::default(),
            host_implementations: Default::default(),
            traits: Default::default(),
            implementations: Default::default(),
            methods: Default::default(),
            inherent_methods: Default::default(),
            native_types: Default::default(),
            structures: Default::default(),
            fields: Default::default(),
            enumerations: Default::default(),
            variants: Default::default(),
        }
    }
}

mod mapping;

impl<I: DefinitionReference> AggregateCatalog<I> {
    pub fn native_type(&self, id: &I) -> Option<&NativeTypeSignature<I>> {
        self.native_types.get(id).map(Arc::as_ref)
    }

    pub fn implementation_constant(&self, implementation: &I, member: &I) -> Option<&I> {
        self.implementation_constants
            .get(implementation)
            .and_then(|members| members.get(member))
            .or_else(|| {
                self.implementation_signature(implementation)
                    .and_then(|implementation| self.trait_(&implementation.trait_type.declaration))
                    .and_then(|contract| contract.associated_consts.get(member))
                    .and_then(|member| member.initializer.as_ref())
            })
    }

    pub fn inherent_methods(&self) -> impl Iterator<Item = &InherentMethodSignature<I>> {
        self.inherent_methods.values().map(AsRef::as_ref)
    }

    pub fn enumerations(&self) -> impl Iterator<Item = &EnumSignature<I>> {
        self.enumerations.values().map(AsRef::as_ref)
    }

    pub fn enumeration(&self, id: &I) -> Option<&EnumSignature<I>> {
        self.enumerations.get(id).map(AsRef::as_ref)
    }

    pub fn variant(&self, id: &I) -> Option<&VariantSignature<I>> {
        let (owner, slot) = self.variants.get(id)?;
        self.enumeration(owner)?.variants.get(*slot)
    }

    pub fn structures(&self) -> impl Iterator<Item = &StructSignature<I>> {
        self.structures.values().map(AsRef::as_ref)
    }

    pub fn structure(&self, id: &I) -> Option<&StructSignature<I>> {
        self.structures.get(id).map(AsRef::as_ref)
    }

    pub fn field(&self, id: &I) -> Option<&FieldSignature<I>> {
        let (owner, slot) = self.fields.get(id)?;
        self.structure(owner)?.fields.get(*slot)
    }
}
