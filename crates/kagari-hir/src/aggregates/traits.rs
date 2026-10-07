//! Trait method/associated-item contracts and bounded applied supertrait expansion.

use crate::{
    aggregates::{AggregateCatalog, implementations::ImplementationSearchError},
    declarations::{Declaration, DeclarationId, Declarations},
    hir::writeability::Writeability,
    lower::LoweredModule,
    native::NativeBinding,
    resolver::resolved::ResolvedName,
    typeck::{FunctionImplementation, GenericBounds, ModuleSignatures, table::ConstraintTarget},
    types::{AssociatedTypeParameters, GenericParameterType, NominalType, TypeId},
};
use kagari_common::{
    cancellation::{CancellationToken, Cancelled},
    identity,
    identity::{DefinitionPath, ModuleIdentity, reference::DefinitionReference},
};
use kagari_source::identity::FileSpan;
use kagari_types::{
    callable::MethodPolicy, collection::CollectionAccess,
    declaration::conversion::ConversionAdapter,
};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

/// A named semantic method parameter without a source-local parameter ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParameter<I: DefinitionReference = DefinitionPath> {
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Declared binding/field mutation policy.
    pub writeability: Writeability,
    /// Checked or recovered semantic value type.
    pub ty: TypeId<I>,
}

/// A canonical trait method contract with default and override policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature<I: DefinitionReference = DefinitionPath> {
    /// Script/native default implementation, or absent for a required method.
    pub default: Option<MethodDefault<I>>,
    /// Installed/checked override policy attached to this declaration.
    pub policy: MethodPolicy,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: I,
    /// Canonical definition identity of the enclosing declaration.
    pub owner: I,
    /// Zero-based member position within the enclosing declaration, not a byte offset.
    pub slot: usize,
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType<I>>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds<I>,
    /// Receiver and explicit parameter contracts in call order.
    pub params: Vec<MethodParameter<I>>,
    /// Semantic result type before call-site substitution.
    pub return_type: TypeId<I>,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration<I>,
}

/// The method declaration owns a default's identity and checked signature.
/// A native default has no script body to instantiate for an implementing type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodDefault<I: DefinitionReference = DefinitionPath> {
    /// Default body belongs to the method's source function.
    Script,
    /// Default implementation comes from an installed native binding.
    Native(NativeBinding<I>),
}

impl MethodSignature {
    fn same_contract(&self, other: &Self) -> bool {
        self.default == other.default
            && self.policy == other.policy
            && self.id == other.id
            && self.owner == other.owner
            && self.slot == other.slot
            && self.name == other.name
            && self.generic_params == other.generic_params
            && self.bounds == other.bounds
            && self.params == other.params
            && self.return_type == other.return_type
    }
}

/// An applied-trait declaration surface, including parents and associated members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitSignature<I: DefinitionReference = DefinitionPath> {
    /// Declared storage-interface access role, when supplied by registration.
    pub storage_access: Option<CollectionAccess>,
    /// Registered conversion adapter policy, if this trait has one.
    pub conversion_adapter: Option<ConversionAdapter<I>>,
    /// Associated family identity to its own binders and constraints.
    pub associated_type_parameters: BTreeMap<I, AssociatedTypeParameters<I>>,
    /// Canonical associated constant identities to their contracts.
    pub associated_consts: BTreeMap<I, AssociatedConstSignature<I>>,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: I,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType<I>>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds<I>,
    /// Declared parent applications before receiver/binder substitution.
    pub supertraits: Vec<NominalType<I>>,
    /// Trait method contracts in logical slot order.
    pub methods: Vec<MethodSignature<I>>,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration<I>,
    /// Associated output identities to their required constraints.
    pub associated_types: BTreeMap<I, Vec<ConstraintTarget<I>>>,
}

/// An associated constant contract and optional canonical default initializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedConstSignature<I: DefinitionReference = DefinitionPath> {
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration<I>,
    /// Checked or recovered semantic value type.
    pub ty: TypeId<I>,
    /// Canonical constant initializer definition, absent for a required value.
    pub initializer: Option<I>,
}

impl AggregateCatalog {
    /// Applied inheritance closure, preserving declaration identities and substitutions.
    /// Repeated paths to the same applied trait are deduplicated; declaration
    /// cycles and expanding paths fail before reaching a body or backend.
    pub fn trait_closure(
        &self,
        interface: &NominalType,
        receiver: &TypeId,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalType>, ImplementationSearchError> {
        trait_inheritance_closure(interface, receiver, cancel, &|id| {
            self.trait_(id).map(|contract| {
                (
                    contract.generic_params.clone(),
                    contract.supertraits.clone(),
                )
            })
        })
    }

    /// Adds applied parent-trait constraints to the supplied bounds, deduplicating equal applications.
    ///
    /// # Errors
    ///
    /// Returns cancellation or bounded/cyclic inheritance expansion failure.
    pub fn expanded_bounds(
        &self,
        bounds: &GenericBounds,
        cancel: &CancellationToken,
    ) -> Result<GenericBounds, ImplementationSearchError> {
        let mut result = bounds.clone();
        for (receiver, constraints) in bounds {
            for constraint in constraints {
                if let ConstraintTarget::Trait(interface) = constraint {
                    for parent in self.trait_closure(interface, receiver, cancel)? {
                        let bound = ConstraintTarget::Trait(parent);
                        let available = result.entry(receiver.clone()).or_default();
                        if !available.contains(&bound) {
                            available.push(bound);
                        }
                    }
                }
            }
        }
        Ok(result)
    }

    pub(super) fn add_traits(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let mut functions = HashMap::new();
        for function in signatures.functions() {
            cancel.check()?;
            functions.insert(function.id, function);
        }
        for item in &lowered.module.traits {
            cancel.check()?;
            let declaration = declarations
                .target(ResolvedName::Trait(item.id))
                .expect("trait declaration");
            let DeclarationId::Definition(id) = &declaration.id else {
                unreachable!("nominal trait");
            };
            let mut generic_params = Vec::new();
            let mut bounds = GenericBounds::new();
            for param in &item.generic_params {
                cancel.check()?;
                let identity = declarations
                    .generic_type(param.id)
                    .expect("trait parameter identity");
                let constraints = param
                    .bounds
                    .iter()
                    .filter_map(|reference| signatures.type_table().constraint(reference.ty))
                    .collect::<Vec<_>>();
                if !constraints.is_empty() {
                    bounds.insert(TypeId::Generic(identity.clone()), constraints);
                }
                generic_params.push(identity);
            }
            let mut methods = Vec::new();
            for (slot, method) in item.methods.iter().enumerate() {
                cancel.check()?;
                let declaration = declarations
                    .target(ResolvedName::Function(method.function))
                    .expect("method declaration");
                let DeclarationId::Definition(method_id) = &declaration.id else {
                    unreachable!("nominal method");
                };
                let function = functions
                    .get(&method.function)
                    .expect("checked method signature");
                let mut params = Vec::new();
                for param in &function.params {
                    cancel.check()?;
                    params.push(MethodParameter {
                        name: param.name.clone(),
                        writeability: param.writeability,
                        ty: param.ty.clone(),
                    });
                }
                self.methods
                    .insert(method_id.clone(), (id.clone(), methods.len()));
                methods.push(MethodSignature {
                    default: match function.implementation.clone() {
                        FunctionImplementation::Native(binding) => {
                            Some(MethodDefault::Native(binding))
                        }
                        FunctionImplementation::Script => Some(MethodDefault::Script),
                        FunctionImplementation::Required => None,
                    },
                    policy: lowered
                        .method_policies
                        .get(&method.function)
                        .copied()
                        .unwrap_or_default(),
                    id: method_id.clone(),
                    owner: id.clone(),
                    slot,
                    name: method.name.clone(),
                    bounds: function.bounds.clone(),
                    generic_params: function.generic_params.clone(),
                    params,
                    return_type: function.return_type.clone(),
                    declaration: declaration.clone(),
                });
            }
            self.traits.insert(
                id.clone(),
                Arc::new(TraitSignature {
                    storage_access: lowered.native_trait_access.get(&item.id).copied(),
                    conversion_adapter: lowered.native_trait_adapters.get(&item.id).cloned(),
                    associated_type_parameters: item
                        .associated_types
                        .iter()
                        .filter(|member| !member.generic_params.is_empty())
                        .filter_map(|member| {
                            let id = identity::associated_type_id(id, &member.name);
                            Some((
                                id.clone(),
                                signatures
                                    .type_table()
                                    .associated_type_parameters(&id)?
                                    .clone(),
                            ))
                        })
                        .collect(),
                    associated_consts: item
                        .associated_consts
                        .iter()
                        .map(|member| {
                            let identity = identity::associated_const_id(id, &member.name);
                            (
                                identity.clone(),
                                AssociatedConstSignature {
                                    declaration: Declaration {
                                        id: DeclarationId::Definition(identity.clone()),
                                        name: member.name.clone(),
                                        location: FileSpan {
                                            file: lowered.source.id(),
                                            revision: lowered.source.revision(),
                                            range: lowered.source_map.type_span(member.name_ref),
                                        },
                                    },
                                    ty: signatures
                                        .type_table()
                                        .type_ref(member.ty)
                                        .map_or(TypeId::Error, |ty| ty.ty.clone()),
                                    initializer: member.initializer.map(|_| identity),
                                },
                            )
                        })
                        .collect(),
                    supertraits: item
                        .supertraits
                        .iter()
                        .filter_map(|reference| {
                            match signatures.type_table().constraint(reference.ty)? {
                                ConstraintTarget::Trait(interface) => Some(interface),
                                _ => None,
                            }
                        })
                        .collect(),
                    associated_types: item
                        .associated_types
                        .iter()
                        .map(|member| {
                            let member = identity::associated_type_id(id, &member.name);
                            let bounds = signatures
                                .type_table()
                                .associated_bounds
                                .get(&member)
                                .cloned()
                                .unwrap_or_default();
                            (member, bounds)
                        })
                        .collect(),
                    id: id.clone(),
                    generic_params,
                    bounds,
                    methods,
                    declaration: declaration.clone(),
                }),
            );
        }
        Ok(())
    }

    pub(super) fn include_traits(
        &self,
        result: &mut Self,
        module: &ModuleIdentity,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let start = DefinitionPath {
            module: module.clone(),
            path: Vec::new(),
        };
        for (id, item) in self
            .traits
            .range(start..)
            .take_while(|(id, _)| &id.module == module)
        {
            cancel.check()?;
            for (index, method) in item.methods.iter().enumerate() {
                cancel.check()?;
                result
                    .methods
                    .insert(method.id.clone(), (id.clone(), index));
            }
            result.traits.insert(id.clone(), item.clone());
        }
        Ok(())
    }

    pub(super) fn same_trait_contracts(&self, other: &Self) -> bool {
        self.traits.len() == other.traits.len()
            && self.traits.iter().all(|(id, a)| {
                other.trait_(id).is_some_and(|b| {
                    a.storage_access == b.storage_access
                        && a.conversion_adapter == b.conversion_adapter
                        && a.generic_params == b.generic_params
                        && a.supertraits == b.supertraits
                        && a.associated_types == b.associated_types
                        && a.associated_type_parameters == b.associated_type_parameters
                        && a.associated_consts.len() == b.associated_consts.len()
                        && a.associated_consts.iter().all(|(id, member)| {
                            b.associated_consts.get(id).is_some_and(|other| {
                                member.ty == other.ty && member.initializer == other.initializer
                            })
                        })
                        && a.bounds == b.bounds
                        && a.methods.len() == b.methods.len()
                        && a.methods
                            .iter()
                            .zip(&b.methods)
                            .all(|(a, b)| a.same_contract(b))
                })
            })
    }
}

/// Collects applied trait parents using caller-supplied declaration lookup and receiver substitution.
///
/// # Errors
///
/// Returns cancellation or `LimitExceeded` for cyclic/expanding inheritance that
/// cannot be admitted within the traversal bounds.
pub fn trait_inheritance_closure(
    interface: &NominalType,
    receiver: &TypeId,
    cancel: &CancellationToken,
    lookup: &impl Fn(&DefinitionPath) -> Option<(Vec<GenericParameterType>, Vec<NominalType>)>,
) -> Result<Vec<NominalType>, ImplementationSearchError> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![(interface.clone(), Vec::<DefinitionPath>::new())];
    while let Some((applied, mut path)) = pending.pop() {
        cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        if path.contains(&applied.declaration) || path.len() >= 64 || result.len() >= 4096 {
            return Err(ImplementationSearchError::LimitExceeded);
        }
        if !seen.insert(applied.clone()) {
            continue;
        }
        let (parameters, supertraits) =
            lookup(&applied.declaration).ok_or(ImplementationSearchError::LimitExceeded)?;
        if parameters.len() != applied.arguments.len() {
            return Err(ImplementationSearchError::LimitExceeded);
        }
        let substitution = parameters
            .into_iter()
            .zip(applied.arguments.iter().cloned())
            .collect();
        path.push(applied.declaration.clone());
        for parent in supertraits.iter().rev() {
            let TypeId::Trait(parent) = TypeId::Trait(parent.clone())
                .with_associated_types(&applied)
                .with_self(&applied.declaration, receiver)
                .instantiate(&substitution)
            else {
                unreachable!("trait parent")
            };
            pending.push((parent, path.clone()));
        }
        result.push(applied);
    }
    Ok(result)
}

mod mapping;

impl<I: DefinitionReference> AggregateCatalog<I> {
    /// Visits trait contracts in definition-key order.
    pub fn traits(&self) -> impl Iterator<Item = &TraitSignature<I>> {
        self.traits.values().map(AsRef::as_ref)
    }

    /// Borrows a trait contract by canonical identity, or `None` if absent.
    pub fn trait_(&self, id: &I) -> Option<&TraitSignature<I>> {
        self.traits.get(id).map(AsRef::as_ref)
    }

    /// Finds a method through the trait-owner/slot reverse index.
    pub fn trait_method(&self, id: &I) -> Option<&MethodSignature<I>> {
        let (owner, slot) = self.methods.get(id)?;
        self.trait_(owner)?.methods.get(*slot)
    }
}

impl<I: DefinitionReference> MethodSignature<I> {
    /// Returns the declaration's checked override permission.
    pub fn allows_override(&self) -> bool {
        self.policy.override_allowed
    }
}
