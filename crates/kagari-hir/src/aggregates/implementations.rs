use super::*;
use crate::types::{GenericParameterType, NominalType};

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

    pub fn concrete_implementation_method(
        &self,
        method: &DefinitionId,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> Option<&DefinitionId> {
        let mut matches = self
            .implementations
            .values()
            .filter(|implementation| {
                implementation.generic_params.is_empty()
                    && implementation.trait_type == *trait_type
                    && implementation.for_type == *receiver
            })
            .filter_map(|implementation| implementation.methods.get(method));
        let result = matches.next()?;
        matches.next().is_none().then_some(result)
    }

    pub fn concrete_implementation_count(
        &self,
        trait_type: &NominalType,
        receiver: &TypeId,
    ) -> usize {
        self.implementations
            .values()
            .filter(|implementation| {
                implementation.generic_params.is_empty()
                    && implementation.trait_type == *trait_type
                    && implementation.for_type == *receiver
            })
            .take(2)
            .count()
    }
}
