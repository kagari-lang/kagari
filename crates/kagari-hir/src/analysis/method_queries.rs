//! Source method completion projects the same checked receiver surfaces as calls.

use crate::{
    aggregates::AggregateCatalog,
    analysis::FileAnalysis,
    declarations::DeclarationId,
    typeck::{ConstraintTarget, GenericBounds, members, type_satisfies_standard_constraint},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::cancellation::CancellationToken;
use std::collections::HashSet;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MethodCompletion {
    pub declaration: DeclarationId,
    pub name: String,
}

impl FileAnalysis {
    /// Source-declared methods available on a complete or incomplete receiver.
    /// Results retain declaration identities for snapshot documentation/navigation.
    pub fn method_completions(&self, offset: usize) -> Vec<MethodCompletion> {
        let Some(receiver) = self.member_receiver_type(offset) else {
            return Vec::new();
        };
        if receiver.is_unresolved() {
            return Vec::new();
        }
        let facts = self.result.facts();
        let empty = GenericBounds::default();
        let assumptions = facts
            .typed
            .functions
            .iter()
            .filter_map(|function| {
                let span = facts.lowered.source_map.function_span(function.id);
                (span.start <= offset && offset <= span.end)
                    .then_some((span.end - span.start, &function.bounds))
            })
            .min_by_key(|(length, _)| *length)
            .map(|(_, bounds)| bounds)
            .unwrap_or(&empty);
        let cancel = CancellationToken::default();
        let query = MemberQuery {
            aggregates: &facts.aggregates,
            receiver: &receiver,
            assumptions,
            cancel: &cancel,
        };
        let mut candidates = Vec::new();
        let mut inherent_names = HashSet::new();
        for method in facts.aggregates.inherent_methods() {
            let function = &method.function;
            let Some(parameter) = function.params.first().filter(|p| p.name == "self") else {
                continue;
            };
            if !method.visibility.allows(
                &method.declaration.module,
                facts.lowered.source.module_identity(),
            ) {
                continue;
            }
            let Some(mut substitution) =
                members::inherent_substitution(&facts.aggregates, method, &receiver, &cancel)
            else {
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
                .normalize_type(&parameter.ty.instantiate(&substitution));
            if (expected.conflicts_with(&receiver) && !receiver.can_weaken_to(&expected))
                || !query.bounds_allow(&function.bounds, &substitution, None)
            {
                continue;
            }
            candidates.push(MethodCompletion {
                declaration: method.site.id.clone(),
                name: function.name.clone(),
            });
        }
        for interface in members::interfaces(&facts.aggregates, &receiver, assumptions, &cancel) {
            let Some(contract) = facts.aggregates.trait_(&interface.declaration) else {
                continue;
            };
            let implementation = facts
                .aggregates
                .concrete_interface_implementation(
                    &interface,
                    &receiver,
                    assumptions,
                    4096,
                    64,
                    &cancel,
                )
                .ok()
                .flatten()
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
                substitution.insert_receiver(contract.id.clone(), receiver.clone());
                for parameter in &method.generic_params {
                    substitution
                        .entry(parameter.clone())
                        .or_insert(TypeId::Unknown);
                }
                if !query.bounds_allow(&method.bounds, &substitution, Some(&interface)) {
                    continue;
                }
                let declaration = implementation
                    .and_then(|implementation| implementation.methods.get(&method.id))
                    .unwrap_or(&method.id)
                    .clone();
                candidates.push(MethodCompletion {
                    declaration: DeclarationId::Definition(declaration),
                    name: method.name.clone(),
                });
            }
        }
        let mut identities = HashSet::new();
        candidates.retain(|candidate| identities.insert(candidate.declaration.clone()));
        candidates.sort_by(|a, b| a.name.cmp(&b.name));
        candidates
    }
}

struct MemberQuery<'a> {
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
    ) -> bool {
        let instantiate = |ty: &TypeId| {
            let applied = if let Some(interface) = interface {
                ty.with_self(&interface.declaration, self.receiver)
                    .instantiate(substitution)
                    .with_associated_types(interface)
            } else {
                ty.instantiate(substitution)
            };
            self.aggregates.normalize_type(&applied)
        };
        bounds.iter().all(|(target, constraints)| {
            let actual = instantiate(target);
            // Method arguments have not been supplied at a completion site.
            if actual.is_unresolved() {
                return true;
            }
            constraints.iter().all(|constraint| match constraint {
                ConstraintTarget::Standard(standard) => {
                    type_satisfies_standard_constraint(&actual, *standard, self.assumptions)
                }
                ConstraintTarget::Trait(required) => {
                    let required = instantiate(&TypeId::Trait(required.clone()));
                    if required.is_unresolved() {
                        return true;
                    }
                    let TypeId::Trait(required) = required else {
                        return false;
                    };
                    self.aggregates
                        .intrinsic_implementation(&required, &actual, self.assumptions)
                        || members::interfaces(
                            self.aggregates,
                            &actual,
                            self.assumptions,
                            self.cancel,
                        )
                        .iter()
                        .any(|available| available.satisfies(&required))
                        || self
                            .aggregates
                            .concrete_interface_implementation(
                                &required,
                                &actual,
                                self.assumptions,
                                4096,
                                64,
                                self.cancel,
                            )
                            .is_ok_and(|selected| selected.is_some())
                }
            })
        })
    }
}
