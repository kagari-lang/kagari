//! Source method completion projects the same checked receiver surfaces as calls.

use crate::{
    aggregates::{
        AggregateCatalog,
        implementations::{ImplementationSearchError, ImplementationSearchLimits},
    },
    analysis::{FileAnalysis, projection::EditorFacts},
    declarations::DeclarationId,
    typeck::{GenericBounds, members},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

/// A visible method candidate presented without executing or invoking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodCompletion {
    /// Identity of the candidate in this file result's retained definition scope.
    pub declaration: DeclarationId,
    /// Source-facing method name to offer at the member access.
    pub name: String,
}

impl FileAnalysis {
    /// Source-declared methods available on a complete or incomplete receiver.
    /// Results retain declaration identities for snapshot documentation/navigation.
    pub fn method_completions(
        &self,
        offset: usize,
        cancel: &CancellationToken,
    ) -> Result<Vec<MethodCompletion>, ImplementationSearchError> {
        methods(
            self.editor_facts(),
            self.definitions(),
            self.member_receiver_type(offset),
            offset,
            cancel,
        )
    }
}

pub(crate) fn methods(
    facts: EditorFacts<'_>,
    definitions: &DefinitionTable,
    receiver: Option<TypeId>,
    offset: usize,
    cancel: &CancellationToken,
) -> Result<Vec<MethodCompletion>, ImplementationSearchError> {
    cancel
        .check()
        .map_err(|_| ImplementationSearchError::Cancelled)?;
    let Some(receiver) = receiver.filter(|receiver| !receiver.is_unresolved()) else {
        return Ok(Vec::new());
    };
    let empty = GenericBounds::default();
    let assumptions = facts
        .typed
        .signatures
        .functions()
        .iter()
        .filter_map(|function| {
            let span = facts.lowered.source_map.function_span(function.id);
            (span.start <= offset && offset <= span.end)
                .then_some((span.end - span.start, &function.bounds))
        })
        .min_by_key(|(length, _)| *length)
        .map(|(_, bounds)| bounds)
        .unwrap_or(&empty);
    let query = MemberQuery {
        definitions,
        aggregates: facts.aggregates,
        receiver: &receiver,
        assumptions,
        cancel,
    };
    let mut candidates = Vec::new();
    let mut inherent_names = HashSet::new();
    for method in facts.aggregates.inherent_methods() {
        cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        let function = &method.function;
        let Some(parameter) = function.params.first().filter(|p| p.name == "self") else {
            continue;
        };
        if !method.visibility.allows(
            definitions
                .resolve(method.declaration)
                .expect("checked method identity")
                .module(),
            facts.lowered.source.module_identity(),
        ) {
            continue;
        }
        let Some(mut substitution) = members::inherent_substitution(
            definitions,
            facts.aggregates,
            method,
            &receiver,
            cancel,
        ) else {
            cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            continue;
        };
        inherent_names.insert(function.name.clone());
        for parameter in &function.generic_params {
            substitution
                .entry(parameter.clone())
                .or_insert(TypeId::Unknown);
        }
        let expected = facts
            .aggregates
            .normalize_type(definitions, &parameter.ty.instantiate(&substitution));
        if (expected.conflicts_with(&receiver) && !receiver.can_weaken_to(&expected))
            || !query.bounds_allow(&function.bounds, &substitution, None)?
        {
            continue;
        }
        candidates.push(MethodCompletion {
            declaration: method.site.id.clone(),
            name: function.name.clone(),
        });
    }
    for interface in members::try_interfaces(
        definitions,
        facts.aggregates,
        &receiver,
        assumptions,
        cancel,
    )? {
        let Some(contract) = facts.aggregates.trait_(&interface.declaration) else {
            continue;
        };
        let implementation = facts
            .aggregates
            .concrete_interface_implementation(
                definitions,
                &interface,
                &receiver,
                assumptions,
                ImplementationSearchLimits {
                    max_checks: 4096,
                    max_depth: 64,
                },
                cancel,
            )?
            .and_then(|(id, _)| facts.aggregates.implementation_signature(&id));
        for method in &contract.methods {
            if inherent_names.contains(&method.name)
                || !method.params.first().is_some_and(|p| p.name == "self")
            {
                continue;
            }
            let mut substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .collect();
            substitution.insert_receiver(contract.id, receiver.clone());
            for parameter in &method.generic_params {
                substitution
                    .entry(parameter.clone())
                    .or_insert(TypeId::Unknown);
            }
            if !query.bounds_allow(&method.bounds, &substitution, Some(&interface))? {
                continue;
            }
            let declaration = *implementation
                .and_then(|implementation| implementation.methods.get(&method.id))
                .unwrap_or(&method.id);
            candidates.push(MethodCompletion {
                declaration: DeclarationId::Definition(declaration),
                name: method.name.clone(),
            });
        }
    }
    let mut identities = HashSet::new();
    candidates.retain(|candidate| identities.insert(candidate.declaration.clone()));
    candidates.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(candidates)
}

struct MemberQuery<'a> {
    definitions: &'a DefinitionTable,
    aggregates: &'a AggregateCatalog,
    receiver: &'a TypeId,
    assumptions: &'a GenericBounds,
    cancel: &'a CancellationToken,
}

impl MemberQuery<'_> {
    fn bounds_allow(
        &self,
        bounds: &GenericBounds,
        substitution: &TypeSubstitution,
        interface: Option<&NominalType>,
    ) -> Result<bool, ImplementationSearchError> {
        let instantiate = |ty: &TypeId| {
            let applied = if let Some(interface) = interface {
                ty.with_self(&interface.declaration, self.receiver)
                    .instantiate(substitution)
                    .with_associated_types(interface, self.receiver)
            } else {
                ty.instantiate(substitution)
            };
            self.aggregates.normalize_type(self.definitions, &applied)
        };
        for (target, constraints) in bounds {
            self.cancel
                .check()
                .map_err(|_| ImplementationSearchError::Cancelled)?;
            let actual = instantiate(target);
            if actual.is_unresolved() {
                continue;
            }
            for required in constraints {
                let required = instantiate(&TypeId::Trait(required.clone()));
                if required.is_unresolved() {
                    continue;
                }
                let TypeId::Trait(required) = required else {
                    return Ok(false);
                };
                if self.aggregates.intrinsic_implementation(
                    self.definitions,
                    &required,
                    &actual,
                    self.assumptions,
                ) {
                    continue;
                }
                if members::try_interfaces(
                    self.definitions,
                    self.aggregates,
                    &actual,
                    self.assumptions,
                    self.cancel,
                )?
                .iter()
                .any(|available| available.satisfies(&required))
                {
                    continue;
                }
                if self
                    .aggregates
                    .concrete_interface_implementation(
                        self.definitions,
                        &required,
                        &actual,
                        self.assumptions,
                        ImplementationSearchLimits {
                            max_checks: 4096,
                            max_depth: 64,
                        },
                        self.cancel,
                    )?
                    .is_none()
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
