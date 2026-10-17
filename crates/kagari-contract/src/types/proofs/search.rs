use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, satisfies, validate_references},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    language::Protocol,
    ty::{
        GenericBound, NominalTy, Ty, inheritance,
        matching::match_pattern,
        substitution::{
            MAX_TYPE_NODES, TypeSubstitution, TypeTransformError, normalize_projections,
        },
    },
};
use std::collections::HashSet;

type ProofKey = (NominalTy, Ty);

#[derive(Default)]
pub(super) struct Search {
    visiting: HashSet<ProofKey>,
    pub(super) defaults: HashSet<ProofKey>,
}

impl ProofCatalog<'_> {
    pub fn holds(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
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
        receiver: &Ty,
        constraints: &[NominalTy],
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        validate_references(receiver, &self.definitions, cancel)?;
        for bound in assumptions {
            validate_references(bound, &self.definitions, cancel)?;
        }
        for constraint in constraints {
            validate_references(constraint, &self.definitions, cancel)?;
        }
        let copier = TypeSubstitution::default();
        copier.apply_bounds(assumptions, &self.definitions, cancel)?;
        if constraints.len() > MAX_TYPE_NODES {
            return Err(TypeTransformError::LimitExceeded);
        }
        let bound = GenericBound {
            ty: copier.apply(receiver, cancel)?,
            constraints: constraints
                .iter()
                .map(|interface| copier.apply_nominal(interface, cancel))
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
        interface: &NominalTy,
        receiver: &Ty,
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
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
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
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<(), TypeTransformError> {
        validate_references(interface, &self.definitions, cancel)?;
        validate_references(receiver, &self.definitions, cancel)?;
        for bound in assumptions {
            validate_references(bound, &self.definitions, cancel)?;
        }
        let copier = TypeSubstitution::default();
        copier.apply_nominal(interface, cancel)?;
        copier.apply(receiver, cancel)?;
        copier.apply_bounds(assumptions, &self.definitions, cancel)?;
        Ok(())
    }

    // An unspecified associated output is still equal to its own projection.
    // This lets a default pass that output as a hidden type argument without
    // asserting any particular concrete implementation for it.
    fn assumption_satisfies(
        &self,
        available: &NominalTy,
        required: &NominalTy,
        receiver: &Ty,
    ) -> bool {
        available.declaration == required.declaration
            && available.arguments == required.arguments
            && required.associated_types.iter().all(|(member, value)| {
                if let Some(actual) = available.associated_types.get(member) {
                    return actual == value;
                }
                let Ty::Projection {
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
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        budget.step(depth)?;
        if assumptions
            .iter()
            .filter(|bound| bound.ty == *receiver)
            .flat_map(|bound| &bound.constraints)
            .any(|available| satisfies(available, interface))
        {
            return Ok(true);
        }
        for bound in assumptions.iter().filter(|bound| bound.ty == *receiver) {
            for available in &bound.constraints {
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
        if let Ty::Projection {
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
            for bound in &output.bounds {
                let bound = substitution.apply(&Ty::Trait(bound.clone()), budget.cancel)?;
                // Resolve outputs from the projection owner's checked assumption.
                // For I: Iterable<Item = T>, I::Iter inherits Iterator<Item = T>,
                // even when the Iter projection names the unqualified contract.
                let Ty::Trait(bound) = normalize_projections(
                    &bound,
                    &|interface, receiver, member, arguments| {
                        if !arguments.is_empty() {
                            return Ok(None);
                        }
                        if interface.declaration == contract.declaration
                            && receiver == owner.as_ref()
                            && let Some(value) = contract.associated_types.get(member)
                        {
                            return Ok(Some(value.clone()));
                        }
                        let mut selected = None;
                        for bound in assumptions.iter().filter(|bound| bound.ty == *receiver) {
                            for available in &bound.constraints {
                                for parent in self.ancestry(available, receiver, budget.cancel)? {
                                    if satisfies(&parent, interface)
                                        && let Some(value) = parent.associated_types.get(member)
                                    {
                                        if selected.as_ref().is_some_and(|old| old != value) {
                                            return Ok(None);
                                        }
                                        selected = Some(value.clone());
                                    }
                                }
                            }
                        }
                        Ok(selected.filter(|value| {
                            value
                                != &Ty::Projection {
                                    receiver: Box::new(receiver.clone()),
                                    interface: Box::new(interface.clone()),
                                    member: *member,
                                    arguments: arguments.to_vec(),
                                }
                        }))
                    },
                    budget.cancel,
                )?
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
        if let Ty::Trait(view) = receiver
            && inheritance::interface_views(
                view,
                receiver,
                budget.cancel,
                &self.definitions,
                &|id| self.contracts.get(id).copied(),
            )?
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
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
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
                let Some(substitution) = match_pattern(
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
                let obligations = substitution.apply_bounds(
                    implementation.bounds(),
                    &self.definitions,
                    budget.cancel,
                )?;
                if self.obligations(&obligations, assumptions, search, budget, depth + 1)? {
                    count += 1;
                }
                if count >= 2 {
                    return Ok(2);
                }
            }

            Ok(count)
        })();
        search.visiting.remove(&key);
        result
    }

    fn fallback(
        &self,
        interface: &NominalTy,
        receiver: &Ty,
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        if let Some(requirements) = intrinsic::requirements(
            interface,
            receiver,
            budget.cancel,
            self.trait_contract(&interface.declaration),
            &self.definitions,
        )? {
            return self.obligations(&requirements, assumptions, search, budget, depth + 1);
        }
        let Some(kind) = Protocol::from_definition(&interface.declaration, &self.definitions)
        else {
            return Ok(false);
        };
        if !interface.arguments.is_empty() || !interface.associated_types.is_empty() {
            return Ok(false);
        }
        self.structural(kind, receiver, assumptions, search, budget, depth)
    }

    pub(super) fn obligations(
        &self,
        obligations: &[GenericBound],
        assumptions: &[GenericBound],
        search: &mut Search,
        budget: &Budget<'_>,
        depth: usize,
    ) -> Result<bool, TypeTransformError> {
        for bound in obligations {
            budget.step(depth)?;
            let actual = self.normalize_with(&bound.ty, budget, depth)?;
            for required in &bound.constraints {
                if !self.prove(required, &actual, assumptions, search, budget, depth)? {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
