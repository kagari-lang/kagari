//! Checked nominal contracts shared by local and imported member access.

use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity::{
        DefinitionKind, ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};

use crate::{
    aggregates::{implementations::ImplementationSignature, traits::TraitSignature},
    declarations::{Declaration, DeclarationId, Declarations},
    hir::{item::adt::Field, writeability::Writeability},
    imports::{ModuleGraph, SourceDeclRef, SourceItem, SourceUnit},
    language::{items::derive_failure_origin, semantics as builtin_traits},
    lower::LoweredModule,
    resolver::resolved::ResolvedName,
    semantic::signatures::TypedFunction,
    typeck::{GenericBounds, ModuleSignatures, constraints},
    types::{GenericParameterType, NominalType, TypeId},
};
use kagari_types::{
    declaration::VariantShape,
    language::{Protocol, role::LangRole},
    visibility::Visibility,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
pub mod implementations;
mod interfaces;
mod native;
pub mod protocols;
mod relationships;
pub mod traits;

/// A canonical struct field contract used for local and imported member access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSignature {
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Canonical definition identity of the enclosing declaration.
    pub owner: DefinitionId,
    /// Zero-based member position within the enclosing declaration, not a byte offset.
    pub slot: usize,
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Declared access boundary checked when selecting this member.
    pub visibility: Visibility,
    /// Declared binding/field mutation policy.
    pub writeability: Writeability,
    /// Checked or recovered semantic value type.
    pub ty: TypeId,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
}

/// An inherent constant selected by its receiver pattern and impl requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InherentConstSignature {
    /// Exact declaring lowering and initializer slot used to project the owner's checked value.
    pub(crate) source: SourceDeclRef,
    pub(crate) site: Declaration,
    pub(crate) owner: TypeId,
    pub(crate) generic_params: Vec<GenericParameterType>,
    pub(crate) bounds: GenericBounds,
    pub(crate) visibility: Visibility,
    pub(crate) ty: TypeId,
}

/// A canonical inherent method, its receiver pattern and typed callable contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InherentMethodSignature {
    /// Qualified source declaration address of the method function.
    pub id: SourceDeclRef,
    /// Canonical method definition identity.
    pub declaration: DefinitionId,
    /// Original method declaration/navigation record.
    pub site: Declaration,
    /// Semantic receiver pattern to which this method belongs.
    pub owner: TypeId,
    /// Declared access boundary checked when selecting this member.
    pub visibility: Visibility,
    /// Checked callable contract, including generic binders and implementation provenance.
    pub function: TypedFunction,
}

/// Shared nominal struct contract with typed field slots and generic requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructSignature {
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
    /// Fields in declaration slot order.
    pub fields: Vec<FieldSignature>,
}

/// Shared Native interface contract and optional static representation facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeTypeSignature {
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
    /// Whether this declaration denotes a Host identity.
    pub is_host: bool,
    /// Declared interface fields; these do not grant executable adapters.
    pub fields: Vec<FieldSignature>,
}

/// Canonical enum variant, payload types and logical variant slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantSignature {
    /// True only for the checked canonical `Result::Err` variant.
    pub reports_failure: bool,
    pub shape: VariantShape,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Canonical definition identity of the enclosing declaration.
    pub owner: DefinitionId,
    /// Zero-based member position within the enclosing declaration, not a byte offset.
    pub slot: usize,
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Payload semantic types in positional order.
    pub payload: Vec<TypeId>,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
}

impl VariantSignature {
    pub(crate) fn named_slots<'a>(
        &self,
        written: impl Iterator<Item = &'a str>,
        rest: bool,
    ) -> Result<Vec<usize>, String> {
        let labels = self
            .shape
            .labels()
            .ok_or("variant does not have named fields")?;
        let by_name: BTreeMap<_, _> = labels
            .iter()
            .enumerate()
            .map(|(slot, label)| (label.as_str(), slot))
            .collect();
        let mut seen = vec![false; labels.len()];
        let mut slots = Vec::new();
        for name in written {
            let slot = *by_name
                .get(name)
                .ok_or_else(|| format!("unknown field `{name}`"))?;
            if seen[slot] {
                return Err(format!("duplicate field `{name}`"));
            }
            seen[slot] = true;
            slots.push(slot);
        }
        if !rest && let Some(slot) = seen.iter().position(|v| !v) {
            return Err(format!("missing field `{}`", labels[slot]));
        }
        Ok(slots)
    }
}

/// Shared nominal enum contract with typed payloads and generic requirements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumSignature {
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
    /// Variants in logical slot order.
    pub variants: Vec<VariantSignature>,
}

/// Type, member and trait implementation signatures across prepared modules.
///
/// Signature preparation collects declarations without walking function bodies. Body
/// checking uses the subset reachable from the current module and checks visibility at use.
/// Signatures may contain error types during recovery.
///
/// To resolve `point.y` by name, look in `Point`'s field list. Given the field ID instead,
/// [`Self::field`] uses a reverse index into that same list:
/// ```text
/// catalog.fields[y_id] = (point_id, 1)  →  catalog.structures[point_id].fields[1]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AggregateCatalog {
    /// Language role → registered definition ID.
    language_items: BTreeMap<LangRole, DefinitionId>,
    /// Trait impl ID → (trait constant ID → overriding initializer ID).
    implementation_constants: BTreeMap<DefinitionId, BTreeMap<DefinitionId, DefinitionId>>,
    /// Trait ID → trait signature, including its declared members and parent traits.
    traits: BTreeMap<DefinitionId, Arc<TraitSignature>>,
    /// Trait impl ID → implemented trait, receiver type pattern, bounds and member mappings.
    implementations: BTreeMap<DefinitionId, Arc<ImplementationSignature>>,
    /// Trait method ID → (trait ID, index in the trait's `methods` list).
    methods: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    /// Inherent method ID → receiver type and method signature.
    inherent_methods: BTreeMap<DefinitionId, Arc<InherentMethodSignature>>,
    inherent_consts: BTreeMap<DefinitionId, Arc<InherentConstSignature>>,
    /// Native type ID → declared type signature and fields.
    native_types: BTreeMap<DefinitionId, Arc<NativeTypeSignature>>,
    /// Struct ID → struct signature with fields in declaration order.
    structures: BTreeMap<DefinitionId, Arc<StructSignature>>,
    /// Field ID → (struct or Native type ID, index in its `fields` list).
    fields: BTreeMap<DefinitionId, (DefinitionId, usize)>,
    /// Enum ID → enum signature with variants in declaration order.
    enumerations: BTreeMap<DefinitionId, Arc<EnumSignature>>,
    /// Variant ID → (enum ID, index in the enum's `variants` list).
    variants: BTreeMap<DefinitionId, (DefinitionId, usize)>,
}

impl AggregateCatalog {
    /// Returns the installed declaration for a supported language protocol, or `None` if its role/trait is absent.
    pub fn language_trait(&self, protocol: Protocol) -> Option<NominalType> {
        let role = LangRole::from_protocol(protocol)?;
        let declaration = *self.language_items.get(&role)?;
        self.trait_(&declaration)?;
        Some(NominalType {
            declaration,
            arguments: vec![],
            associated_types: BTreeMap::new(),
        })
    }

    /// Checks whether intrinsic protocol rules apply under the supplied generic bounds.
    pub fn intrinsic_implementation(
        &self,
        definitions: &DefinitionTable,
        interface: &NominalType,
        ty: &TypeId,
        bounds: &GenericBounds,
    ) -> bool {
        builtin_traits::intrinsic_applies(definitions, interface, ty, Some(self), bounds)
    }

    pub(crate) fn add_module(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        cancel.check()?;
        if !lowered.source.module_identity().within_path_limit() {
            return Ok(());
        }
        self.language_items.extend(
            declarations
                .language_items
                .iter()
                .map(|(role, id)| (*role, *id)),
        );
        for item in &lowered.module.native_types {
            cancel.check()?;
            let Some(declaration) = declarations.target(ResolvedName::NativeType(item.id)) else {
                continue;
            };
            let DeclarationId::Definition(id) = &declaration.id else {
                continue;
            };
            let fields =
                self.collect_fields(&item.fields, *id, declarations, signatures, cancel)?;
            self.native_types.insert(
                *id,
                Arc::new(NativeTypeSignature {
                    id: *id,
                    generic_params: declarations.parameters_of(id),
                    bounds: signatures
                        .type_bounds(id)
                        .expect("checked native type constraints")
                        .clone(),
                    declaration: declaration.clone(),
                    is_host: item.is_host,
                    fields,
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
            let fields =
                self.collect_fields(&structure.fields, *id, declarations, signatures, cancel)?;
            self.structures.insert(
                *id,
                Arc::new(StructSignature {
                    id: *id,
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
                self.variants.insert(*variant_id, (*id, variants.len()));
                variants.push(VariantSignature {
                    reports_failure: false,
                    shape: variant.shape.clone(),
                    id: *variant_id,
                    owner: *id,
                    slot: variant.id.slot(),
                    name: variant.name.clone(),
                    payload,
                    declaration: declaration.clone(),
                });
            }
            let mut signature = EnumSignature {
                id: *id,
                generic_params: declarations.parameters_of(id),
                bounds: signatures
                    .type_bounds(id)
                    .expect("checked type constraints")
                    .clone(),
                declaration: declaration.clone(),
                variants,
            };
            if enumeration.visibility == Visibility::Public {
                derive_failure_origin(&mut signature, declarations.definitions());
            }
            self.enumerations.insert(*id, Arc::new(signature));
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
                        declarations.definitions().lookup_child(
                            contract.trait_type.declaration,
                            DefinitionKind::Const,
                            &member.name,
                            0,
                        )?,
                        *declarations.definition(ResolvedName::Const(initializer))?,
                    ))
                })
                .collect();
            self.implementation_constants.insert(*owner, constants);
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
            for member in &implementation.associated_consts {
                cancel.check()?;
                let Some(id) = member.initializer else {
                    continue;
                };
                let Some(site) = declarations.target(ResolvedName::Const(id)) else {
                    continue;
                };
                let DeclarationId::Definition(key) = site.id else {
                    continue;
                };
                let Some(ty) = signatures.type_table().type_ref(member.ty) else {
                    continue;
                };
                self.inherent_consts.insert(
                    key,
                    Arc::new(InherentConstSignature {
                        source: SourceDeclRef {
                            unit: SourceUnit::of(lowered, declarations.definitions()),
                            item: SourceItem::Const(id),
                        },
                        site: site.clone(),
                        owner: owner.clone(),
                        ty: ty.ty.clone(),
                        generic_params: implementation
                            .generic_params
                            .iter()
                            .filter_map(|id| declarations.generic_type(*id))
                            .collect(),
                        bounds: constraints::implementation_bounds(
                            &lowered.module,
                            implementation,
                            declarations,
                            signatures.type_table(),
                        ),
                        visibility: lowered.module.constant(id).visibility,
                    }),
                );
            }
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
                    *declaration,
                    Arc::new(InherentMethodSignature {
                        id: SourceDeclRef {
                            unit: SourceUnit::of(lowered, declarations.definitions()),
                            item: SourceItem::Function(method.function),
                        },
                        declaration: *declaration,
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

    fn collect_fields(
        &mut self,
        fields: &[Field],
        owner: DefinitionId,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<Vec<FieldSignature>, Cancelled> {
        let mut result = Vec::new();
        for field in fields {
            cancel.check()?;
            let Some(declaration) = declarations.field(field.id) else {
                continue;
            };
            let DeclarationId::Definition(id) = declaration.id else {
                continue;
            };
            self.fields.insert(id, (owner, result.len()));
            result.push(FieldSignature {
                id,
                owner,
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
        Ok(result)
    }

    pub(crate) fn for_module(
        &self,
        root: &ModuleIdentity,
        graph: &ModuleGraph,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<Self, Cancelled> {
        let mut reachable = BTreeSet::new();
        let mut pending = vec![
            graph
                .catalog
                .definitions
                .lookup_module(root)
                .expect("registered module"),
        ];
        while let Some(module) = pending.pop() {
            cancel.check()?;
            if !reachable.insert(module) {
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
        self.include_traits(&mut result, &reachable, definitions, cancel)?;
        for (id, implementation) in &self.implementations {
            cancel.check()?;
            if !reachable.contains(
                &definitions
                    .module_of(*id)
                    .expect("scoped implementation identity"),
            ) {
                continue;
            }
            result.implementations.insert(*id, implementation.clone());
            if let Some(constants) = self.implementation_constants.get(id) {
                result
                    .implementation_constants
                    .insert(*id, constants.clone());
            }
        }
        for (id, constant) in &self.inherent_consts {
            cancel.check()?;
            if reachable.contains(
                &definitions
                    .module_of(*id)
                    .expect("scoped constant identity"),
            ) {
                result.inherent_consts.insert(*id, constant.clone());
            }
        }
        for (id, method) in &self.inherent_methods {
            cancel.check()?;
            if reachable.contains(&definitions.module_of(*id).expect("scoped method identity")) {
                result.inherent_methods.insert(*id, method.clone());
            }
        }
        for (id, native_type) in &self.native_types {
            cancel.check()?;
            if reachable.contains(&definitions.module_of(*id).expect("scoped type identity")) {
                for (slot, field) in native_type.fields.iter().enumerate() {
                    cancel.check()?;
                    result.fields.insert(field.id, (*id, slot));
                }
                result.native_types.insert(*id, native_type.clone());
            }
        }
        for (id, structure) in &self.structures {
            cancel.check()?;
            if !reachable.contains(&definitions.module_of(*id).expect("scoped struct identity")) {
                continue;
            }
            for (index, field) in structure.fields.iter().enumerate() {
                cancel.check()?;
                result.fields.insert(field.id, (*id, index));
            }
            result.structures.insert(*id, structure.clone());
        }
        for (id, enumeration) in &self.enumerations {
            cancel.check()?;
            if !reachable.contains(&definitions.module_of(*id).expect("scoped enum identity")) {
                continue;
            }
            for (index, variant) in enumeration.variants.iter().enumerate() {
                cancel.check()?;
                result.variants.insert(variant.id, (*id, index));
            }
            result.enumerations.insert(*id, enumeration.clone());
        }
        Ok(result)
    }
}

mod mapping;

impl AggregateCatalog {
    /// Visits Native type interfaces, including declaration-only inputs.
    pub fn native_types(&self) -> impl Iterator<Item = &NativeTypeSignature> {
        self.native_types.values().map(AsRef::as_ref)
    }

    /// Borrows a Native type contract by canonical definition identity.
    pub fn native_type(&self, id: &DefinitionId) -> Option<&NativeTypeSignature> {
        self.native_types.get(id).map(Arc::as_ref)
    }

    /// Finds an implementation constant override, falling back to the trait member's default initializer.
    pub fn implementation_constant(
        &self,
        implementation: &DefinitionId,
        member: &DefinitionId,
    ) -> Option<&DefinitionId> {
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

    /// Inherent constant definitions reachable from this module; access still requires visibility and a matching receiver.
    pub(crate) fn inherent_consts(
        &self,
    ) -> impl Iterator<Item = (DefinitionId, &InherentConstSignature)> {
        self.inherent_consts
            .iter()
            .map(|(id, item)| (*id, item.as_ref()))
    }

    /// Original declaration site of a directly selected inherent constant.
    pub fn inherent_const_site(&self, id: DefinitionId) -> Option<&Declaration> {
        Some(&self.inherent_consts.get(&id)?.site)
    }

    /// Visits inherent method contracts in definition-key order.
    pub fn inherent_methods(&self) -> impl Iterator<Item = &InherentMethodSignature> {
        self.inherent_methods.values().map(AsRef::as_ref)
    }

    /// Visits enum contracts in definition-key order.
    pub fn enumerations(&self) -> impl Iterator<Item = &EnumSignature> {
        self.enumerations.values().map(AsRef::as_ref)
    }

    /// Borrows an enum contract, or `None` if the definition is not in this catalog.
    pub fn enumeration(&self, id: &DefinitionId) -> Option<&EnumSignature> {
        self.enumerations.get(id).map(AsRef::as_ref)
    }

    /// Finds a canonical variant through its owner/slot reverse index.
    pub fn variant(&self, id: &DefinitionId) -> Option<&VariantSignature> {
        let (owner, slot) = self.variants.get(id)?;
        self.enumeration(owner)?.variants.get(*slot)
    }

    /// Visits struct contracts in definition-key order.
    pub fn structures(&self) -> impl Iterator<Item = &StructSignature> {
        self.structures.values().map(AsRef::as_ref)
    }

    /// Borrows a struct contract, or `None` if the definition is not in this catalog.
    pub fn structure(&self, id: &DefinitionId) -> Option<&StructSignature> {
        self.structures.get(id).map(AsRef::as_ref)
    }

    /// Finds a canonical field through its owner/slot reverse index.
    pub fn field(&self, id: &DefinitionId) -> Option<&FieldSignature> {
        let (owner, slot) = self.fields.get(id)?;
        if let Some(structure) = self.structure(owner) {
            structure.fields.get(*slot)
        } else {
            self.native_type(owner)?.fields.get(*slot)
        }
    }
}
