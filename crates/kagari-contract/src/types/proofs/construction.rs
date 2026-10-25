//! Exact implementation admission for method-independent construction evidence.
use crate::{
    callable::{
        construction::{
            evidence::{ConstructionRequirement, ConstructionSelection, ConstructionWitness},
            requirements::constructions_valid,
        },
        witness::required_operations,
    },
    native_import::callables::normalize_requirement,
    types::{
        ConcreteFunctionIdentity,
        proofs::{ProofCatalog, satisfies},
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::ty::{
    GenericBound, GenericParam, Ty,
    inheritance::interface_upcast_targets,
    substitution::{TypeSubstitution, TypeTransformError},
};

impl ProofCatalog<'_> {
    pub(crate) fn construction_matches(
        &self,
        witness: &ConstructionWitness,
        parameters: &[GenericParam],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let ConstructionSelection::Declared {
            implementation,
            operations,
            constructions,
        } = &witness.selection
        else {
            return Ok(false);
        };
        let Some(bounds) = self.selected_construction_bounds(
            &witness.requirement,
            implementation,
            assumptions,
            cancel,
        )?
        else {
            return Ok(false);
        };
        let required = required_operations(
            &bounds,
            &|id| self.trait_contract(id),
            cancel,
            &self.definitions,
        )?
        .iter()
        .map(|required| normalize_requirement(required, self, cancel))
        .collect::<Result<Vec<_>, _>>()?;
        if required.len() != operations.len()
            || required.iter().any(|required| {
                operations
                    .iter()
                    .filter(|operation| operation.requirement() == required)
                    .count()
                    != 1
            })
        {
            return Ok(false);
        }
        for operation in operations {
            if !operation.valid(self, parameters, assumptions, cancel)? {
                return Ok(false);
            }
        }
        constructions_valid(
            constructions,
            &bounds,
            self,
            parameters,
            assumptions,
            cancel,
        )
    }

    pub(super) fn selected_construction_bounds(
        &self,
        requirement: &ConstructionRequirement,
        implementation: &ConcreteFunctionIdentity,
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<Option<Vec<GenericBound>>, TypeTransformError> {
        cancel.check().map_err(|_| TypeTransformError::Cancelled)?;
        let Some(table) = self
            .implementations
            .iter()
            .chain(self.construction_bridges.iter())
            .find(|table| *table.declaration() == implementation.declaration)
        else {
            return Ok(None);
        };
        if table.parameters().len() != implementation.arguments.len() {
            return Ok(None);
        }
        let Some(interface) = table.interface() else {
            return Ok(None);
        };
        let mut substitution = TypeSubstitution::default();
        for (parameter, argument) in table.parameters().iter().zip(&implementation.arguments) {
            substitution.bind(&parameter.owner, parameter.position, argument);
        }
        if self.normalize(&substitution.apply(table.receiver(), cancel)?, cancel)?
            != requirement.receiver
        {
            return Ok(None);
        }
        let Ty::Trait(interface) = self.normalize(
            &Ty::Trait(substitution.apply_nominal(interface, cancel)?),
            cancel,
        )?
        else {
            return Ok(None);
        };
        if table.is_bridge()
            && !self.holds(&interface, &requirement.receiver, assumptions, cancel)?
        {
            return Ok(None);
        }
        let views = interface_upcast_targets(
            &interface,
            &requirement.receiver,
            cancel,
            &self.definitions,
            &|id| self.trait_contract(id),
        )?;
        if !views
            .iter()
            .any(|view| satisfies(view, &requirement.interface))
        {
            return Ok(None);
        }
        let bounds = substitution.apply_bounds(table.bounds(), &self.definitions, cancel)?;
        for bound in &bounds {
            if !self.constraints_hold(&bound.ty, &bound.constraints, assumptions, cancel)? {
                return Ok(None);
            }
        }
        Ok(Some(bounds))
    }
}

#[cfg(test)]
mod tests;
