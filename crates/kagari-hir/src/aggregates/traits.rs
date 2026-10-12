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
    identity::{
        ModuleIdentity,
        table::{DefinitionId, DefinitionTable},
    },
};
use kagari_types::{callable::MethodPolicy, declaration::conversion::ConversionAdapter};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

/// A named semantic method parameter without a source-local parameter ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParameter {
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Declared binding/field mutation policy.
    pub writeability: Writeability,
    /// Checked or recovered semantic value type.
    pub ty: TypeId,
}

/// A canonical trait method contract with default and override policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature {
    /// Script/native default implementation, or absent for a required method.
    pub default: Option<MethodDefault>,
    /// Installed/checked override policy attached to this declaration.
    pub policy: MethodPolicy,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Canonical definition identity of the enclosing declaration.
    pub owner: DefinitionId,
    /// Zero-based member position within the enclosing declaration, not a byte offset.
    pub slot: usize,
    /// Member/type spelling for lookup and diagnostics; identity is stored separately.
    pub name: String,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Receiver and explicit parameter contracts in call order.
    pub params: Vec<MethodParameter>,
    /// Semantic result type before call-site substitution.
    pub return_type: TypeId,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
}

/// The method declaration owns a default's identity and checked signature.
/// A native default owns a checked forwarding body in its generated trait source.
/// Lowering that proof selects a native entry; implementing types need no script
/// body instance or extra script call frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MethodDefault {
    /// Default body belongs to the method's source function.
    Script,
    /// Default implementation comes from an installed native binding.
    Native(NativeBinding),
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
pub struct TraitSignature {
    /// Registered conversion adapter policy, if this trait has one.
    pub conversion_adapter: Option<ConversionAdapter>,
    /// Associated family identity to its own binders and constraints.
    pub associated_type_parameters: BTreeMap<DefinitionId, AssociatedTypeParameters>,
    /// Canonical associated constant identities to their contracts.
    pub associated_consts: BTreeMap<DefinitionId, AssociatedConstSignature>,
    /// Canonical semantic definition identity used as the catalog key.
    pub id: DefinitionId,
    /// Generic binders in declaration order, identified by owner and position.
    pub generic_params: Vec<GenericParameterType>,
    /// Checked generic constraints used during application/implementation selection.
    pub bounds: GenericBounds,
    /// Declared parent applications before receiver/binder substitution.
    pub supertraits: Vec<NominalType>,
    /// Trait method contracts in logical slot order.
    pub methods: Vec<MethodSignature>,
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
    /// Associated output identities to their required constraints.
    pub associated_types: BTreeMap<DefinitionId, Vec<ConstraintTarget>>,
}

/// An associated constant contract and optional canonical default initializer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedConstSignature {
    /// Semantic identity and authoritative source navigation site.
    pub declaration: Declaration,
    /// Checked or recovered semantic value type.
    pub ty: TypeId,
    /// Canonical constant initializer definition, absent for a required value.
    pub initializer: Option<DefinitionId>,
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
                self.methods.insert(*method_id, (*id, methods.len()));
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
                    id: *method_id,
                    owner: *id,
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
                *id,
                Arc::new(TraitSignature {
                    conversion_adapter: lowered.native_trait_adapters.get(&item.id).cloned(),
                    associated_type_parameters: item
                        .associated_types
                        .iter()
                        .filter(|member| !member.generic_params.is_empty())
                        .filter_map(|member| {
                            let DeclarationId::Definition(id) =
                                declarations.associated_type(member.name_ref)?.id
                            else {
                                return None;
                            };
                            Some((
                                id,
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
                            let declaration = declarations
                                .associated_const(member.name_ref)
                                .expect("collected associated constant");
                            let DeclarationId::Definition(identity) = declaration.id else {
                                unreachable!("associated constant definition");
                            };
                            (
                                identity,
                                AssociatedConstSignature {
                                    declaration: declaration.clone(),
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
                            let DeclarationId::Definition(member) = declarations
                                .associated_type(member.name_ref)
                                .expect("collected associated type")
                                .id
                            else {
                                unreachable!("associated type definition");
                            };
                            let bounds = signatures
                                .type_table()
                                .associated_bounds
                                .get(&member)
                                .cloned()
                                .unwrap_or_default();
                            (member, bounds)
                        })
                        .collect(),
                    id: *id,
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
        reachable: &BTreeSet<ModuleIdentity>,
        definitions: &DefinitionTable,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        for (id, item) in &self.traits {
            cancel.check()?;
            if !reachable.contains(
                definitions
                    .resolve(*id)
                    .expect("scoped trait identity")
                    .module(),
            ) {
                continue;
            }
            for (index, method) in item.methods.iter().enumerate() {
                cancel.check()?;
                result.methods.insert(method.id, (*id, index));
            }
            result.traits.insert(*id, item.clone());
        }
        Ok(())
    }

    pub(super) fn same_trait_contracts(&self, other: &Self) -> bool {
        self.traits.len() == other.traits.len()
            && self.traits.iter().all(|(id, a)| {
                other.trait_(id).is_some_and(|b| {
                    a.conversion_adapter == b.conversion_adapter
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
    lookup: &impl Fn(&DefinitionId) -> Option<(Vec<GenericParameterType>, Vec<NominalType>)>,
) -> Result<Vec<NominalType>, ImplementationSearchError> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();
    let mut pending = vec![(interface.clone(), Vec::<DefinitionId>::new())];
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
        path.push(applied.declaration);
        for parent in supertraits.iter().rev() {
            let TypeId::Trait(parent) = TypeId::Trait(parent.clone())
                .with_associated_types(&applied, &TypeId::SelfType(applied.declaration))
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

impl AggregateCatalog {
    /// Visits trait contracts in definition-key order.
    pub fn traits(&self) -> impl Iterator<Item = &TraitSignature> {
        self.traits.values().map(AsRef::as_ref)
    }

    /// Borrows a trait contract by canonical identity, or `None` if absent.
    pub fn trait_(&self, id: &DefinitionId) -> Option<&TraitSignature> {
        self.traits.get(id).map(AsRef::as_ref)
    }

    /// Finds a method through the trait-owner/slot reverse index.
    pub fn trait_method(&self, id: &DefinitionId) -> Option<&MethodSignature> {
        let (owner, slot) = self.methods.get(id)?;
        self.trait_(owner)?.methods.get(*slot)
    }
}

impl MethodSignature {
    /// Returns the declaration's checked override permission.
    pub fn allows_override(&self) -> bool {
        self.policy.override_allowed
    }
}
