//! Checked declared interface applications, shared by inference and branch joins.

use crate::{
    aggregates::{
        AggregateCatalog,
        implementations::{ImplementationSearchError, ImplementationSearchLimits},
    },
    typeck::{GenericBounds, inference, table::ConstraintTarget},
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
        if let TypeId::Trait(interface) = ty {
            return self.interface_closure(definitions, interface, ty, cancel);
        }
        let mut roots = Vec::new();
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
            for bound in assumptions.get(receiver.as_ref()).into_iter().flatten() {
                if let ConstraintTarget::Trait(available) = bound
                    && available.declaration == applied.declaration
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
                    .map(|bound| match bound {
                        ConstraintTarget::Standard(value) => ConstraintTarget::Standard(*value),
                        ConstraintTarget::Trait(value) => {
                            let TypeId::Trait(value) = TypeId::Trait(value.clone())
                                .instantiate(&substitution)
                                .with_associated_types(&applied, receiver)
                            else {
                                unreachable!("associated trait bound");
                            };
                            ConstraintTarget::Trait(value)
                        }
                    }),
            );
        }
        roots.extend(bounds.into_iter().filter_map(|bound| match bound {
            ConstraintTarget::Trait(ty) => Some(ty),
            ConstraintTarget::Standard(_) => None,
        }));
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

    /// Find the most-specific applications accepted by every input.
    /// All inputs participate before dominance or ambiguity is decided.
    pub(crate) fn common_interfaces(
        &self,
        definitions: &DefinitionTable,
        inputs: &[TypeId],
        assumptions: &GenericBounds,
        cancel: &CancellationToken,
    ) -> Result<Vec<NominalType>, ImplementationSearchError> {
        let available = inputs
            .iter()
            .map(|input| self.declared_interfaces(definitions, input, assumptions, cancel))
            .collect::<Result<Vec<_>, _>>()?;
        let Some(first) = available.first() else {
            return Ok(Vec::new());
        };
        // An existing operand's interface is a checked one-way target, including
        // its explicitly chosen associated constraints. Do not invent erased
        // associated outputs merely to make otherwise distinct candidates meet.
        let mut common = Vec::new();
        for input in inputs {
            if let TypeId::Trait(target) = input
                && available
                    .iter()
                    .all(|views| views.iter().any(|view| view.satisfies(target)))
                && !common.contains(target)
            {
                common.push(target.clone());
            }
        }
        if common.is_empty() {
            common = first.clone();
            for views in &available[1..] {
                common.retain(|candidate| views.contains(candidate));
            }
        }
        let mut dominated = Vec::new();
        for candidate in &common {
            let parents = self.interface_closure(
                definitions,
                candidate,
                &TypeId::Trait(candidate.clone()),
                cancel,
            )?;
            for other in &common {
                if other != candidate
                    && parents.iter().any(|parent| parent.satisfies(other))
                    && !dominated.contains(other)
                {
                    dominated.push(other.clone());
                }
            }
        }
        common.retain(|candidate| !dominated.contains(candidate));
        Ok(common)
    }
}
