//! Projection equality during exact search uses only caller evidence or applicable impls.

use super::{ImplementationSearchError, ImplementationSignature, SearchBudget};
use crate::{
    aggregates::AggregateCatalog,
    collections::HashSet,
    language::semantics as traits,
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::identity::{
    DefinitionKind,
    table::{DefinitionId, DefinitionTable},
};
use kagari_types::language::Protocol;

impl ImplementationSignature {
    pub(super) fn project(
        &self,
        member: &DefinitionId,
        arguments: &[TypeId],
        substitution: &TypeSubstitution,
    ) -> Option<TypeId> {
        if arguments.is_empty() {
            Some(
                self.trait_type
                    .associated_types
                    .get(member)?
                    .instantiate(substitution),
            )
        } else {
            self.associated_type_families
                .get(member)?
                .apply(substitution, arguments)
        }
    }
}

impl AggregateCatalog {
    pub(super) fn primitive_projection(
        &self,
        definitions: &DefinitionTable,
        interface: &NominalType,
        receiver: &TypeId,
        member: DefinitionId,
    ) -> Option<TypeId> {
        if Some(member)
            == definitions.lookup_child(
                interface.declaration,
                DefinitionKind::AssociatedType,
                "Output",
                0,
            )
        {
            return traits::intrinsic_output(definitions, interface, receiver);
        }
        None
    }

    pub(super) fn normalize_for_search(
        &self,
        ty: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
        budget: &mut SearchBudget<'_>,
    ) -> Result<TypeId, ImplementationSearchError> {
        if !ty.contains_projection() {
            return Ok(ty.clone());
        }
        budget.check_candidate()?;
        budget.nested(|budget| {
            let mut status = Ok(());
            let ty = ty.map_children(|child| {
                if status.is_err() {
                    return TypeId::Error;
                }
                match self.normalize_for_search(child, visiting, budget) {
                    Ok(child) => child,
                    Err(error) => {
                        status = Err(error);
                        TypeId::Error
                    }
                }
            });
            status?;
            match self.projection_output(&ty, visiting, budget)? {
                Some(output) if output != ty => {
                    self.normalize_for_search(&output, visiting, budget)
                }
                Some(_) => Ok(TypeId::Error),
                None => Ok(ty),
            }
        })
    }

    fn projection_output(
        &self,
        ty: &TypeId,
        visiting: &mut HashSet<(NominalType, TypeId)>,
        budget: &mut SearchBudget<'_>,
    ) -> Result<Option<TypeId>, ImplementationSearchError> {
        let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = ty
        else {
            return Ok(None);
        };
        if arguments.is_empty() {
            let known = interface
                .associated_types
                .get(member)
                .cloned()
                .or_else(|| {
                    budget
                        .assumptions
                        .get(receiver.as_ref())?
                        .iter()
                        .filter(|bound| bound.satisfies(interface))
                        .find_map(|bound| bound.associated_types.get(member).cloned())
                })
                .or_else(|| {
                    self.primitive_projection(budget.definitions, interface, receiver, *member)
                });
            if let Some(output) = known {
                // A reflexive caller equality names the symbolic projection itself.
                return Ok((output != *ty).then_some(output));
            }
            if let TypeId::Trait(view) = receiver.as_ref() {
                return Ok(self
                    .interface_closure(budget.definitions, view, receiver, budget.cancel)?
                    .into_iter()
                    .find(|parent| parent.satisfies(interface))
                    .and_then(|parent| parent.associated_types.get(member).cloned()));
            }
        }
        let applications = self.explicit_applications(interface, receiver, visiting, budget)?;
        match applications.as_slice() {
            [(implementation, substitution)] => {
                Ok(implementation.project(member, arguments, substitution))
            }
            [] if arguments.is_empty()
                && let Some(iterator) =
                    traits::iterator_requirement(budget.definitions, interface, receiver)
                && self.assumed_or_declared(&iterator, receiver, visiting, budget)? =>
            {
                let assumptions = [(receiver.as_ref().clone(), vec![iterator])]
                    .into_iter()
                    .collect();
                Ok(traits::iteration_outputs(
                    budget.definitions,
                    Protocol::Iterable,
                    receiver,
                    None,
                    &assumptions,
                )
                .and_then(|outputs| outputs.get(member).cloned()))
            }
            [] => Ok(None),
            _ => Ok(Some(TypeId::Error)),
        }
    }
}
