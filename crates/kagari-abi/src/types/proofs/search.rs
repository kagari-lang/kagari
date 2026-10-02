use crate::{
    language::{Protocol, primitive as intrinsic},
    types::{
        AbiType, ConstraintAbi, GenericBoundAbi, NominalAbiType, inheritance, matching,
        proofs::{Budget, ProofCatalog, host_application, satisfies},
        substitution::{
            MAX_TYPE_NODES, TypeSubstitution, TypeTransformError, resolve_associated_outputs,
        },
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

    // An unspecified associated output is still equal to its own projection.
    // This lets a default pass that output as a hidden type argument without
    // asserting any particular concrete implementation for it.
    fn assumption_satisfies(
        &self,
        available: &NominalAbiType,
        required: &NominalAbiType,
        receiver: &AbiType,
    ) -> bool {
        available.declaration == required.declaration
            && available.arguments == required.arguments
            && required.associated_types.iter().all(|(member, value)| {
                if let Some(actual) = available.associated_types.get(member) {
                    return actual == value;
                }
                let AbiType::Projection {
                    receiver: projected,
                    interface,
                    member: output,
                    arguments,
                } = value
                else {
                    return false;
                };
                projected.as_ref() == receiver
                    && output == member
                    && arguments.is_empty()
                    && interface.declaration == available.declaration
                    && interface.arguments == available.arguments
                    && interface
                        .associated_types
                        .iter()
                        .all(|(key, ty)| available.associated_types.get(key) == Some(ty))
                    && self
                        .contracts
                        .get(&available.declaration)
                        .is_some_and(|contract| {
                            contract.associated_types.iter().any(|output| {
                                output.declaration == *member && output.generic_params.is_empty()
                            })
                        })
            })
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
        for bound in assumptions.iter().filter(|bound| bound.ty == *receiver) {
            for constraint in &bound.constraints {
                let ConstraintAbi::Trait(available) = constraint else {
                    continue;
                };
                budget.step(depth)?;
                if self
                    .ancestry(available, receiver, budget.cancel)?
                    .iter()
                    .any(|parent| self.assumption_satisfies(parent, interface, receiver))
                {
                    return Ok(true);
                }
            }
        }
        if let AbiType::Projection {
            receiver: owner,
            interface: contract,
            member,
            arguments,
        } = receiver
            && let Some(declaration) = self.contracts.get(&contract.declaration)
            && let Some(output) = declaration
                .associated_types
                .iter()
                .find(|output| output.declaration == *member)
            && output.generic_params.len() == arguments.len()
            && self.prove(contract, owner, assumptions, search, budget, depth + 1)?
        {
            let mut substitution =
                TypeSubstitution::for_owner(&contract.declaration, &contract.arguments);
            substitution.bind_receiver(&contract.declaration, owner);
            for (parameter, argument) in output.generic_params.iter().zip(arguments) {
                substitution.bind(&parameter.owner, parameter.position, argument);
            }
            for constraint in &output.bounds {
                let ConstraintAbi::Trait(bound) = constraint else {
                    continue;
                };
                let bound = substitution.apply(&AbiType::Trait(bound.clone()), budget.cancel)?;
                let AbiType::Trait(bound) =
                    resolve_associated_outputs(&bound, contract, budget.cancel)?
                else {
                    unreachable!("associated trait bound");
                };
                if self
                    .ancestry(&bound, receiver, budget.cancel)?
                    .iter()
                    .any(|bound| satisfies(bound, interface))
                {
                    return Ok(true);
                }
            }
        }
        if let AbiType::Trait(view) = receiver
            && inheritance::interface_views(view, receiver, budget.cancel, &|id| {
                self.contracts.get(id).copied()
            })?
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
            for implementation in &self.implementations {
                budget.step(depth)?;
                let Some(substitution) = matching::match_pattern(
                    implementation
                        .pattern()
                        .ok_or(TypeTransformError::InvalidContract)?,
                    interface,
                    receiver,
                    budget.cancel,
                )?
                else {
                    continue;
                };
                if implementation.parameters().iter().any(|parameter| {
                    substitution
                        .parameter(&parameter.owner, parameter.position)
                        .is_none()
                }) {
                    continue;
                }
                let obligations =
                    substitution.apply_bounds(implementation.bounds(), budget.cancel)?;
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
        let Some(kind) = Protocol::from_id(&interface.declaration) else {
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
