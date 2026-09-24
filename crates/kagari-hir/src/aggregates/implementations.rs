use super::*;
use crate::types::TypeSubstitution;
use crate::types::{GenericParameterType, NominalType};
use std::collections::HashSet;

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
        let mut matches = self.implementations.values().filter_map(|implementation| {
            let matched = self.implementation_matches(
                implementation,
                trait_type,
                receiver,
                &mut HashSet::new(),
            )?;
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
        self.implementations
            .values()
            .filter(|implementation| {
                self.implementation_matches(
                    implementation,
                    trait_type,
                    receiver,
                    &mut HashSet::new(),
                )
                .is_some()
            })
            .take(2)
            .count()
    }

    fn implementation_matches(
        &self,
        implementation: &ImplementationSignature,
        trait_type: &NominalType,
        receiver: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
    ) -> Option<TypeSubstitution> {
        let matched = crate::typeck::match_implementation(
            &implementation.trait_type,
            trait_type,
            &implementation.for_type,
            receiver,
            &implementation.generic_params,
        )?;
        let key = (trait_type.clone(), receiver.clone());
        if !visiting.insert(key.clone()) {
            return None;
        }
        let holds = implementation
            .bounds
            .iter()
            .all(|(parameter, constraints)| {
                let Some(actual) = matched.get(parameter) else {
                    return false;
                };
                constraints.iter().all(|constraint| match constraint {
                    crate::typeck::ConstraintTarget::Standard(standard) => {
                        crate::typeck::type_satisfies_standard_constraint(
                            actual,
                            *standard,
                            &Default::default(),
                        )
                    }
                    crate::typeck::ConstraintTarget::Trait(required) => {
                        let required = required.instantiate(&matched);
                        self.implementations.values().any(|candidate| {
                            self.implementation_matches(candidate, &required, actual, visiting)
                                .is_some()
                        })
                    }
                })
            });
        visiting.remove(&key);
        holds.then_some(matched)
    }
}
