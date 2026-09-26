use super::*;
use crate::{typeck::ConstraintTarget, types::GenericParameterType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodParameter {
    pub name: String,
    pub writeability: Writeability,
    pub ty: TypeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodSignature {
    pub has_default: bool,
    pub id: DefinitionId,
    pub owner: DefinitionId,
    pub slot: usize,
    pub name: String,
    pub generic_params: Vec<GenericParameterType>,
    pub bounds: crate::typeck::GenericBounds,
    pub params: Vec<MethodParameter>,
    pub return_type: TypeId,
    pub declaration: Declaration,
}

impl MethodSignature {
    fn same_contract(&self, other: &Self) -> bool {
        self.has_default == other.has_default
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraitSignature {
    pub id: DefinitionId,
    pub generic_params: Vec<GenericParameterType>,
    pub bounds: crate::typeck::GenericBounds,
    pub supertraits: Vec<crate::types::NominalType>,
    pub methods: Vec<MethodSignature>,
    pub declaration: Declaration,
    pub associated_types: BTreeMap<DefinitionId, Vec<ConstraintTarget>>,
}

impl AggregateCatalog {
    /// Applied inheritance closure, preserving declaration identities and substitutions.
    /// Repeated paths to the same applied trait are deduplicated; declaration
    /// cycles and expanding paths fail before reaching a body or backend.
    pub fn trait_closure(
        &self,
        interface: &crate::types::NominalType,
        receiver: &TypeId,
        cancel: &CancellationToken,
    ) -> Result<Vec<crate::types::NominalType>, super::ImplementationSearchError> {
        trait_inheritance_closure(interface, receiver, cancel, &|id| {
            self.trait_(id).map(|contract| {
                (
                    contract.generic_params.clone(),
                    contract.supertraits.clone(),
                )
            })
        })
    }

    pub fn expanded_bounds(
        &self,
        bounds: &crate::typeck::GenericBounds,
        cancel: &CancellationToken,
    ) -> Result<crate::typeck::GenericBounds, super::ImplementationSearchError> {
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
    pub fn traits(&self) -> impl Iterator<Item = &TraitSignature> {
        self.traits.values().map(AsRef::as_ref)
    }

    pub fn trait_(&self, id: &DefinitionId) -> Option<&TraitSignature> {
        self.traits.get(id).map(AsRef::as_ref)
    }

    pub fn trait_method(&self, id: &DefinitionId) -> Option<&MethodSignature> {
        let (owner, slot) = self.methods.get(id)?;
        self.trait_(owner)?.methods.get(*slot)
    }

    pub(super) fn add_traits(
        &mut self,
        lowered: &LoweredModule,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        let mut functions = std::collections::HashMap::new();
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
            let mut bounds = crate::typeck::GenericBounds::new();
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
                    has_default: method.has_default,
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
                            let member = crate::types::associated_type_id(id, &member.name);
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
        let start = DefinitionId {
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
                    a.generic_params == b.generic_params
                        && a.supertraits == b.supertraits
                        && a.associated_types == b.associated_types
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

pub fn trait_inheritance_closure(
    interface: &crate::types::NominalType,
    receiver: &TypeId,
    cancel: &CancellationToken,
    lookup: &impl Fn(
        &DefinitionId,
    ) -> Option<(Vec<GenericParameterType>, Vec<crate::types::NominalType>)>,
) -> Result<Vec<crate::types::NominalType>, super::ImplementationSearchError> {
    use super::ImplementationSearchError as Error;
    let mut result = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut pending = vec![(interface.clone(), Vec::<DefinitionId>::new())];
    while let Some((applied, mut path)) = pending.pop() {
        cancel.check().map_err(|_| Error::Cancelled)?;
        if path.contains(&applied.declaration) || path.len() >= 64 || result.len() >= 4096 {
            return Err(Error::LimitExceeded);
        }
        if !seen.insert(applied.clone()) {
            continue;
        }
        let (parameters, supertraits) = lookup(&applied.declaration).ok_or(Error::LimitExceeded)?;
        if parameters.len() != applied.arguments.len() {
            return Err(Error::LimitExceeded);
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
