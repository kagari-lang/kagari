use super::*;
use crate::types::TypeSubstitution;
use crate::types::{GenericParameterType, NominalType};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImplementationSearchError {
    Cancelled,
    LimitExceeded,
}

struct SearchBudget<'a> {
    checks_left: usize,
    depth: usize,
    max_depth: usize,
    cancel: &'a CancellationToken,
    assumptions: &'a crate::typeck::GenericBounds,
}

impl SearchBudget<'_> {
    fn check_candidate(&mut self) -> Result<(), ImplementationSearchError> {
        self.cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        if self.checks_left == 0 {
            return Err(ImplementationSearchError::LimitExceeded);
        }
        self.checks_left -= 1;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplementationSignature {
    pub id: DefinitionId,
    pub trait_type: NominalType,
    pub for_type: TypeId,
    pub generic_params: Vec<GenericParameterType>,
    pub bounds: crate::typeck::GenericBounds,
    /// Trait method identity to implementation method identity.
    pub methods: BTreeMap<DefinitionId, DefinitionId>,
}

impl AggregateCatalog {
    pub fn implementation_signature(&self, id: &DefinitionId) -> Option<&ImplementationSignature> {
        self.implementations.get(id).map(AsRef::as_ref)
    }
    pub fn normalize_type(&self, ty: &TypeId) -> TypeId {
        crate::typeck::associated::normalize(ty, &|interface, receiver, member| {
            let mut matches = self.implementations.values().filter_map(|implementation| {
                let substitution = crate::typeck::match_implementation(
                    &implementation.trait_type,
                    interface,
                    &implementation.for_type,
                    receiver,
                    &implementation.generic_params,
                )?;
                Some(
                    implementation
                        .trait_type
                        .associated_types
                        .get(member)?
                        .instantiate(&substitution),
                )
            });
            let result = matches.next()?;
            matches.next().is_none().then_some(result)
        })
    }
    /// Build a catalog from already validated executable implementation records.
    /// Duplicate declaration identities are rejected rather than overwritten.
    pub fn from_implementation_signatures(
        signatures: impl IntoIterator<Item = ImplementationSignature>,
    ) -> Option<Self> {
        let mut catalog = Self::default();
        for signature in signatures {
            if catalog
                .implementations
                .insert(signature.id.clone(), Arc::new(signature))
                .is_some()
            {
                return None;
            }
        }
        Some(catalog)
    }

    pub(crate) fn implementations(&self) -> impl Iterator<Item = &ImplementationSignature> {
        self.implementations.values().map(AsRef::as_ref)
    }

    pub(crate) fn overlapping_implementations(
        &self,
    ) -> Vec<(&ImplementationSignature, &ImplementationSignature)> {
        let mut by_trait: BTreeMap<&DefinitionId, Vec<&ImplementationSignature>> = BTreeMap::new();
        let mut overlaps = Vec::new();
        for implementation in self.implementations.values() {
            let previous = by_trait
                .entry(&implementation.trait_type.declaration)
                .or_default();
            if let Some(conflict) = previous.iter().copied().find(|candidate| {
                (candidate.trait_type.arguments == implementation.trait_type.arguments
                    || !candidate
                        .trait_type
                        .arguments
                        .iter()
                        .all(TypeId::is_concrete)
                    || !implementation
                        .trait_type
                        .arguments
                        .iter()
                        .all(TypeId::is_concrete))
                    && crate::typeck::possibly_overlapping_impls(
                        &candidate.for_type,
                        &implementation.for_type,
                    )
            }) {
                overlaps.push((conflict, implementation.as_ref()));
            }
            previous.push(implementation);
        }
        overlaps
    }

    pub(crate) fn add_implementations(
        &mut self,
        declarations: &Declarations,
        signatures: &ModuleSignatures,
        cancel: &CancellationToken,
    ) -> Result<(), Cancelled> {
        for (id, trait_type, for_type, generic_params, bounds, methods) in
            signatures.type_table().implementation_entries()
        {
            cancel.check()?;
            let mut method_identities = BTreeMap::new();
            for (trait_method, function) in methods {
                cancel.check()?;
                let Some(declaration) = declarations.definition(ResolvedName::Function(*function))
                else {
                    continue;
                };
                method_identities.insert(trait_method.clone(), declaration.clone());
            }
            self.implementations.insert(
                id.clone(),
                Arc::new(ImplementationSignature {
                    id: id.clone(),
                    trait_type: trait_type.clone(),
                    for_type: for_type.clone(),
                    generic_params: generic_params.to_vec(),
                    bounds: bounds.clone(),
                    methods: method_identities,
                }),
            );
        }
        Ok(())
    }

    pub fn implementation_method(
        &self,
        method: &DefinitionId,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<(&DefinitionId, Vec<TypeId>)> {
        let cancel = CancellationToken::default();
        let mut budget = SearchBudget {
            checks_left: usize::MAX,
            depth: 0,
            max_depth: usize::MAX,
            cancel: &cancel,
            assumptions: &Default::default(),
        };
        let mut matches = self.implementations.values().filter_map(|implementation| {
            let matched = self
                .implementation_matches(
                    implementation,
                    trait_type,
                    receiver,
                    &mut HashSet::new(),
                    &mut budget,
                )
                .ok()??;
            let arguments = implementation
                .generic_params
                .iter()
                .map(|parameter| matched.get(parameter).cloned())
                .collect::<Option<Vec<_>>>()?;
            Some((implementation.methods.get(method)?, arguments))
        });
        let result = matches.next()?;
        matches.next().is_none().then_some(result)
    }

    pub fn implementation_count(&self, trait_type: &NominalType, receiver: &TypeId) -> usize {
        self.implementation_count_bounded(
            trait_type,
            receiver,
            usize::MAX,
            usize::MAX,
            &CancellationToken::default(),
        )
        .unwrap_or(0)
    }

    pub fn concrete_interface_implementation(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
        assumptions: &crate::typeck::GenericBounds,
        max_checks: usize,
        max_depth: usize,
        cancel: &CancellationToken,
    ) -> Result<Option<(DefinitionId, Vec<TypeId>)>, ImplementationSearchError> {
        let mut budget = SearchBudget {
            checks_left: max_checks,
            depth: 0,
            max_depth,
            cancel,
            assumptions,
        };
        let mut selected = None;
        for implementation in self.implementations.values() {
            if let Some(matched) = self.implementation_matches(
                implementation,
                trait_type,
                receiver,
                &mut HashSet::new(),
                &mut budget,
            )? {
                if selected.is_some() {
                    return Ok(None);
                }
                let Some(arguments) = implementation
                    .generic_params
                    .iter()
                    .map(|parameter| matched.get(parameter).cloned())
                    .collect::<Option<Vec<_>>>()
                else {
                    return Ok(None);
                };
                selected = Some((implementation.id.clone(), arguments));
            }
        }
        Ok(selected)
    }

    pub fn implementation_count_bounded(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
        max_checks: usize,
        max_depth: usize,
        cancel: &CancellationToken,
    ) -> Result<usize, ImplementationSearchError> {
        let mut budget = SearchBudget {
            checks_left: max_checks,
            depth: 0,
            max_depth,
            cancel,
            assumptions: &Default::default(),
        };
        let mut count = 0;
        for implementation in self.implementations.values() {
            if self
                .implementation_matches(
                    implementation,
                    trait_type,
                    receiver,
                    &mut HashSet::new(),
                    &mut budget,
                )?
                .is_some()
            {
                count += 1;
                if count == 2 {
                    break;
                }
            }
        }
        Ok(count)
    }

    fn implementation_matches(
        &self,
        implementation: &ImplementationSignature,
        trait_type: &NominalType,
        receiver: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
        budget: &mut SearchBudget<'_>,
    ) -> Result<Option<TypeSubstitution>, ImplementationSearchError> {
        budget.check_candidate()?;
        let Some(matched) = crate::typeck::match_implementation(
            &implementation.trait_type,
            trait_type,
            &implementation.for_type,
            receiver,
            &implementation.generic_params,
        ) else {
            return Ok(None);
        };
        let key = (trait_type.clone(), receiver.clone());
        if !visiting.insert(key.clone()) {
            return Ok(None);
        }
        let holds = (|| {
            for (parameter, constraints) in &implementation.bounds {
                let actual = self.normalize_type(&parameter.instantiate(&matched));
                for constraint in constraints {
                    budget
                        .cancel
                        .check()
                        .map_err(|_| ImplementationSearchError::Cancelled)?;
                    let satisfied = match constraint {
                        crate::typeck::ConstraintTarget::Standard(standard) => {
                            crate::typeck::type_satisfies_standard_constraint(
                                &actual,
                                *standard,
                                budget.assumptions,
                            )
                        }
                        crate::typeck::ConstraintTarget::Trait(required) => {
                            if budget.depth >= budget.max_depth {
                                return Err(ImplementationSearchError::LimitExceeded);
                            }
                            let required = required.instantiate(&matched);
                            if budget.assumptions.get(&actual).is_some_and(|bounds| {
                                bounds.iter().any(|bound| {
                                    matches!(bound, crate::typeck::ConstraintTarget::Trait(available)
                                        if available.satisfies(&required))
                                })
                            }) {
                                continue;
                            }
                            budget.depth += 1;
                            let found = (|| {
                                for candidate in self.implementations.values() {
                                    if self
                                        .implementation_matches(
                                            candidate, &required, &actual, visiting, budget,
                                        )?
                                        .is_some()
                                    {
                                        return Ok(true);
                                    }
                                }
                                Ok(false)
                            })();
                            budget.depth -= 1;
                            found?
                        }
                    };
                    if !satisfied {
                        return Ok(false);
                    }
                }
            }
            Ok(true)
        })();
        visiting.remove(&key);
        Ok(holds?.then_some(matched))
    }
}

#[cfg(test)]
mod search_tests {
    use super::*;
    use crate::{
        typeck::ConstraintTarget,
        types::{BuiltinType, TypeId},
    };
    use kagari_common::identity::{
        DefinitionKind, DefinitionPathSegment, ModuleIdentity, PackageId,
    };

    fn definition(kind: DefinitionKind, name: &str) -> DefinitionId {
        DefinitionId {
            module: ModuleIdentity {
                package: PackageId("pkg".into()),
                path: vec!["module".into()],
            },
            path: vec![DefinitionPathSegment {
                kind,
                name: name.into(),
                occurrence: 0,
            }],
        }
    }

    #[test]
    fn bounded_trait_search_reuses_generic_matching_and_stops_growth() {
        let marker = definition(DefinitionKind::Trait, "Marker");
        let owner = definition(DefinitionKind::Impl, "");
        let parameter = GenericParameterType {
            owner: owner.clone(),
            position: 0,
            name: "T".into(),
        };
        let applied = |argument| NominalType {
            associated_types: Default::default(),
            declaration: marker.clone(),
            arguments: vec![argument],
        };
        let signature = ImplementationSignature {
            id: owner,
            trait_type: applied(TypeId::Generic(parameter.clone())),
            for_type: TypeId::Generic(parameter.clone()),
            generic_params: vec![parameter.clone()],
            bounds: Default::default(),
            methods: Default::default(),
        };
        let catalog = AggregateCatalog::from_implementation_signatures([signature.clone()])
            .expect("unique declaration");
        let actual = TypeId::Builtin(BuiltinType::I32);
        let required = applied(actual.clone());
        let cancel = CancellationToken::default();
        assert_eq!(
            catalog.implementation_count_bounded(&required, &actual, 1, 4, &cancel),
            Ok(1)
        );
        assert_eq!(
            catalog.implementation_count_bounded(&required, &actual, 0, 4, &cancel),
            Err(ImplementationSearchError::LimitExceeded)
        );
        cancel.cancel();
        assert_eq!(
            catalog.implementation_count_bounded(&required, &actual, 1, 4, &cancel),
            Err(ImplementationSearchError::Cancelled)
        );

        let mut chained = signature;
        let next = definition(DefinitionKind::Trait, "Next");
        chained.bounds.insert(
            TypeId::Generic(parameter.clone()),
            vec![ConstraintTarget::Trait(NominalType {
                associated_types: Default::default(),
                declaration: next.clone(),
                arguments: vec![TypeId::Generic(parameter.clone())],
            })],
        );
        let next_parameter = GenericParameterType {
            owner: definition(DefinitionKind::Impl, "next"),
            position: 0,
            name: "U".into(),
        };
        let next_signature = ImplementationSignature {
            id: next_parameter.owner.clone(),
            trait_type: NominalType {
                associated_types: Default::default(),
                declaration: next,
                arguments: vec![TypeId::Generic(next_parameter.clone())],
            },
            for_type: TypeId::Generic(next_parameter.clone()),
            generic_params: vec![next_parameter.clone()],
            bounds: Default::default(),
            methods: Default::default(),
        };
        let catalog =
            AggregateCatalog::from_implementation_signatures([chained, next_signature]).unwrap();
        assert_eq!(
            catalog.implementation_count_bounded(
                &required,
                &actual,
                100,
                0,
                &CancellationToken::default(),
            ),
            Err(ImplementationSearchError::LimitExceeded)
        );
        assert_eq!(
            catalog.implementation_count_bounded(
                &required,
                &actual,
                100,
                1,
                &CancellationToken::default(),
            ),
            Ok(1)
        );
    }
}
