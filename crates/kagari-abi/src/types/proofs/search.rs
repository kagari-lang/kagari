use crate::{
    standard::{intrinsic, traits::StandardTrait},
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType, matching,
        proofs::{Budget, ProofCatalog, host_application, satisfies},
        substitution::{MAX_TYPE_NODES, TypeSubstitution, TypeTransformError},
    },
};
use kagari_common::cancellation::CancellationToken;
use std::collections::HashSet;

type ProofKey = (NominalAbiType, AbiType);
#[derive(Default)]
pub(super) struct Search {
    visiting: HashSet<ProofKey>,
    pub(super) defaults: HashSet<ProofKey>,
}

impl ProofCatalog<'_> {
    pub fn holds(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        self.preflight(interface, receiver, assumptions, cancel)?;
        self.prove(
            interface,
            receiver,
            assumptions,
            &mut Search::default(),
            &Budget::new(cancel),
            0,
        )
    }

    pub fn constraints_hold(
        &self,
        receiver: &AbiType,
        constraints: &[ConstraintAbi],
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let copier = TypeSubstitution::default();
        copier.apply_bounds(assumptions, cancel)?;
        if constraints.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        let bound = GenericBoundAbi {
            ty: copier.apply(receiver, cancel)?,
            constraints: constraints
                .iter()
                .map(|constraint| {
                    Ok(match constraint {
                        ConstraintAbi::Standard(kind) => ConstraintAbi::Standard(*kind),
                        ConstraintAbi::Trait(interface) => {
                            ConstraintAbi::Trait(copier.apply_nominal(interface, cancel)?)
                        }
                    })
                })
                .collect::<Result<_, TypeTransformError>>()?,
        };
        self.obligations(
            &[bound],
            assumptions,
            &mut Search::default(),
            &Budget::new(cancel),
            0,
        )
    }

    pub fn has_explicit_implementation(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        self.preflight(interface, receiver, &[], cancel)?;
        Ok(self.explicit(
            interface,
            receiver,
            &[],
            &mut Search::default(),
            &Budget::new(cancel),
            0,
        )? != 0)
    }

    pub fn implementation_count(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<usize, TypeTransformError> {
        self.preflight(interface, receiver, assumptions, cancel)?;
        let budget = Budget::new(cancel);
        let mut search = Search::default();
        let count = self.explicit(interface, receiver, assumptions, &mut search, &budget, 0)?;
        if count != 0 {
            return Ok(count);
        }
        Ok(usize::from(self.fallback(
            interface,
            receiver,
            assumptions,
            &mut search,
            &budget,
            0,
        )?))
    }

    fn preflight(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        cancel: &CancellationToken,
    ) -> Result<(), TypeTransformError> {
        let copier = TypeSubstitution::default();
        copier.apply_nominal(interface, cancel)?;
        copier.apply(receiver, cancel)?;
        copier.apply_bounds(assumptions, cancel)?;
        Ok(())
    }

    pub(super) fn prove(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        if assumptions.iter().filter(|bound| bound.ty == *receiver).flat_map(|bound| &bound.constraints).any(|bound| matches!(bound, ConstraintAbi::Trait(available) if satisfies(available, interface))) { return Ok(true); }
        if let AbiType::Trait(view) = receiver
            && self
                .ancestry(view, receiver, budget.cancel)?
                .iter()
                .any(|parent| satisfies(parent, interface))
        {
            return Ok(true);
        }
        if search
            .visiting
            .contains(&(interface.clone(), receiver.clone()))
        {
            return Ok(false);
        }
        if search
            .defaults
            .contains(&(interface.clone(), receiver.clone()))
        {
            return Ok(true);
        }
        let count = self.explicit(interface, receiver, assumptions, search, budget, depth)?;
        if count != 0 {
            return Ok(count == 1);
        }
        self.fallback(interface, receiver, assumptions, search, budget, depth)
    }

    pub(super) fn explicit(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<usize, TypeTransformError> {
        budget.step(depth)?;
        let key = (interface.clone(), receiver.clone());
        if !search.visiting.insert(key.clone()) {
            return Ok(0);
        }
        let result = (|| {
            let mut count = 0;
            for table in &self.tables {
                budget.step(depth)?;
                let Some(substitution) =
                    matching::match_implementation(table, interface, receiver, budget.cancel)?
                else {
                    continue;
                };
                if table.generic_params.iter().any(|parameter| {
                    substitution
                        .parameter(&parameter.owner, parameter.position)
                        .is_none()
                }) {
                    continue;
                }
                let obligations = substitution.apply_bounds(&table.bounds, budget.cancel)?;
                if self.obligations(&obligations, assumptions, search, budget, depth + 1)? {
                    count += 1;
                }
                if count >= 2 {
                    return Ok(2);
                }
            }
            if let AbiType::Host(id) = receiver {
                for host in &self.hosts {
                    if host.id != *id {
                        continue;
                    }
                    for implementation in &host.trait_implementations {
                        budget.step(depth)?;
                        if satisfies(&host_application(implementation), interface) {
                            count += 1;
                        }
                        if count >= 2 {
                            return Ok(2);
                        }
                    }
                }
            }
            Ok(count)
        })();
        search.visiting.remove(&key);
        result
    }

    fn fallback(
        &self,
        interface: &NominalAbiType,
        receiver: &AbiType,
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        if let Some(requirements) = intrinsic::requirements(interface, receiver, budget.cancel)? {
            return self.obligations(&requirements, assumptions, search, budget, depth + 1);
        }
        let Some(kind) = StandardTrait::from_id(&interface.declaration) else {
            return Ok(false);
        };
        if !interface.arguments.is_empty() || !interface.associated_types.is_empty() {
            return Ok(false);
        }
        self.structural(kind, receiver, assumptions, search, budget, depth)
    }

    pub(super) fn obligations(
        &self,
        obligations: &[GenericBoundAbi],
        assumptions: &[GenericBoundAbi],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        for bound in obligations {
            budget.step(depth)?;
            let actual = self.normalize_with(&bound.ty, budget, depth)?;
            for constraint in &bound.constraints {
                let holds = match constraint {
                    ConstraintAbi::Standard(required) => self.standard_constraint(
                        &actual,
                        *required,
                        assumptions,
                        search,
                        budget,
                        depth,
                    )?,
                    ConstraintAbi::Trait(required) => {
                        self.prove(required, &actual, assumptions, search, budget, depth)?
                    }
                };
                if !holds {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
