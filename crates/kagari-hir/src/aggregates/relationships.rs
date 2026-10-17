//! Checked declared interface applications, shared by member lookup and checked applications.

use crate::{
    aggregates::{
        AggregateCatalog,
        implementations::{ImplementationSearchError, ImplementationSearchLimits},
    },
    typeck::{GenericBounds, inference},
    types::{NominalType, TypeId, TypeSubstitution},
};
use kagari_common::{cancellation::CancellationToken, identity::table::DefinitionTable};

impl AggregateCatalog {
    /// Enumerate declared applications and their parents. Intrinsic operator
    /// availability is not an implicit conversion to an interface value.
    pub(crate) fn declared_interfaces(
        &self,
        definitions: &DefinitionTable,
        ty: &TypeId,
        assumptions: &GenericBounds,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalType>, ImplementationSearchError> {
        cancel
            .check()
            .map_err(|_| ImplementationSearchError::Cancelled)?;
        let mut roots = if let TypeId::Trait(interface) = ty {
            self.interface_closure(definitions, interface, ty, cancel)?
        } else {
            Vec::new()
        };
        if !matches!(
            ty,
            TypeId::Generic(_) | TypeId::SelfType(_) | TypeId::Projection { .. }
        ) {
            for implementation in self.implementations() {
                cancel
                    .check()
                    .map_err(|_| ImplementationSearchError::Cancelled)?;
                let mut substitution = TypeSubstitution::default();
                inference::infer(
                    &implementation.for_type,
                    ty,
                    &implementation.generic_params,
                    &mut substitution,
                    cancel,
                    None,
                )
                .map_err(|_| ImplementationSearchError::Cancelled)?;
                if implementation.for_type.instantiate(&substitution) != *ty {
                    continue;
                }
                let applied = implementation.trait_type.instantiate(&substitution);
                if self
                    .concrete_interface_implementation(
                        definitions,
                        &applied,
                        ty,
                        assumptions,
                        ImplementationSearchLimits {
                            max_checks: 100_000,
                            max_depth: 64,
                        },
                        cancel,
                    )?
                    .is_some()
                    && !roots.contains(&applied)
                {
                    roots.push(applied);
                }
            }
            // Concrete receivers prove each parent through its own implementation.
            // Keep those implementations' associated outputs: a dynamic parent's
            // erased output is not another concrete method candidate.
            return Ok(roots);
        }
        let mut bounds = assumptions
            .get(ty)
            .into_iter()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        if let TypeId::Projection {
            receiver,
            interface,
            member,
            arguments,
        } = ty
            && let Some(contract) = self.trait_(&interface.declaration)
        {
            let mut applied = (**interface).clone();
            for available in assumptions.get(receiver.as_ref()).into_iter().flatten() {
                if available.declaration == applied.declaration
                    && available.arguments == applied.arguments
                {
                    applied
                        .associated_types
                        .extend(available.associated_types.clone());
                }
            }
            let mut substitution: TypeSubstitution = contract
                .generic_params
                .iter()
                .cloned()
                .zip(interface.arguments.iter().cloned())
                .collect();
            substitution.insert_receiver(interface.declaration, (**receiver).clone());
            if let Some(inputs) = contract.associated_type_parameters.get(member) {
                substitution.extend(
                    inputs
                        .parameters
                        .iter()
                        .cloned()
                        .zip(arguments.iter().cloned()),
                );
            }
            bounds.extend(
                contract
                    .associated_types
                    .get(member)
                    .into_iter()
                    .flatten()
                    .map(|value| {
                        let TypeId::Trait(value) = TypeId::Trait(value.clone())
                            .instantiate(&substitution)
                            .with_associated_types(&applied, receiver)
                        else {
                            unreachable!("associated trait bound");
                        };
                        value
                    }),
            );
        }
        roots.extend(bounds);
        let mut expanded = Vec::new();
        for root in roots {
            for parent in self.trait_closure(&root, ty, cancel)? {
                if !expanded.contains(&parent) {
                    expanded.push(parent);
                }
            }
        }
        Ok(expanded)
    }
}
