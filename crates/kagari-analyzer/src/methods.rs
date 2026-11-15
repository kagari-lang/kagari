use crate::file::FileQueries;

use kagari_common::cancellation::CancellationToken;
use kagari_hir::{
    aggregates::implementations::ImplementationSearchLimits,
    declarations::DeclarationId,
    typeck::{GenericBounds, members},
    types::{TypeId, TypeSubstitution},
};
use std::collections::HashSet;

#[cfg(test)]
mod tests;

/// A visible method candidate presented without executing or invoking it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionCandidate {
    /// Identity of the candidate in this file result's retained definition scope.
    pub declaration: DeclarationId,
    /// Source-facing method name to offer at the member access.
    pub name: String,
}

impl FileQueries<'_> {
    /// Source-declared methods available on a complete or incomplete receiver.
    /// Results retain declaration identities for snapshot documentation/navigation.
    pub fn method_completions(&self, offset: usize) -> Vec<CompletionCandidate> {
        let Some(receiver) = self.member_receiver_type(offset) else {
            return Vec::new();
        };
        if receiver.is_unresolved() {
            return Vec::new();
        }
        let facts = self.analysis.result().facts();
        let empty = GenericBounds::default();
        let assumptions = facts
            .typed
            .signatures()
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
        let cancel = CancellationToken::default();
        let query = members::MemberQuery {
            definitions: self.analysis.definitions(),
            aggregates: &facts.aggregates,
            receiver: &receiver,
            assumptions,
            cancel: &cancel,
        };
        let mut candidates = Vec::new();
        let mut inherent_names = HashSet::new();
        for method in facts.aggregates.inherent_methods() {
            let function = &method.function;
            let Some(mut substitution) = members::inherent_method_substitution(
                self.analysis.definitions(),
                &facts.aggregates,
                method,
                &receiver,
                facts.lowered.source.module_identity(),
                &cancel,
            ) else {
                continue;
            };
            let parameter = function.params.first().expect("eligible receiver");
            inherent_names.insert(function.name.clone());
            for parameter in &function.generic_params {
                substitution
                    .entry(parameter.clone())
                    .or_insert(TypeId::Unknown);
            }
            let expected = facts.aggregates.normalize_type(
                self.analysis.definitions(),
                &parameter.ty.instantiate(&substitution),
            );
            if (expected.conflicts_with(&receiver) && !receiver.can_weaken_to(&expected))
                || !query.bounds_allow(&function.bounds, &substitution, None, true)
            {
                continue;
            }
            candidates.push(CompletionCandidate {
                declaration: method.site.id.clone(),
                name: function.name.clone(),
            });
        }
        for interface in members::interfaces(
            self.analysis.definitions(),
            &facts.aggregates,
            &receiver,
            assumptions,
            &cancel,
        ) {
            let Some(contract) = facts.aggregates.trait_(&interface.declaration) else {
                continue;
            };
            let implementation = facts
                .aggregates
                .concrete_interface_implementation(
                    self.analysis.definitions(),
                    &interface,
                    &receiver,
                    assumptions,
                    ImplementationSearchLimits {
                        max_checks: 4096,
                        max_depth: 64,
                    },
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
                substitution.insert_receiver(contract.id, receiver.clone());
                for parameter in &method.generic_params {
                    substitution
                        .entry(parameter.clone())
                        .or_insert(TypeId::Unknown);
                }
                if !query.bounds_allow(&method.bounds, &substitution, Some(&interface), true) {
                    continue;
                }
                let declaration = *implementation
                    .and_then(|implementation| implementation.methods.get(&method.id))
                    .unwrap_or(&method.id);
                candidates.push(CompletionCandidate {
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
