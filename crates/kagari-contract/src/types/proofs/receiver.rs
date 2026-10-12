//! Authenticate conditional receiver selection without treating it as a bound.
use crate::{
    callable::{
        receiver::{
            dispatch::{ReceiverCase, receiver_child},
            object_receiver_may_overlap,
        },
        witness::SharedMethodWitness,
    },
    types::{
        ConcreteFunctionIdentity,
        proofs::{Budget, ProofCatalog},
    },
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    language::Protocol,
    ty::{GenericBound, Ty, matching::match_pattern, substitution::TypeTransformError},
};

impl ProofCatalog<'_> {
    /// Exact executable template that a dispatch case must retain.
    pub fn receiver_dispatch_target(
        &self,
        case: &ReceiverCase,
    ) -> Option<ConcreteFunctionIdentity> {
        let implementation = self
            .implementations
            .iter()
            .find(|implementation| *implementation.declaration() == case.implementation)?;
        Some(ConcreteFunctionIdentity {
            declaration: case.implementation,
            arguments: implementation
                .parameters()
                .iter()
                .map(|parameter| parameter.as_type())
                .collect(),
        })
    }

    /// Authenticate an exhaustive set of potentially matching implementation
    /// identities. Omission never authorizes a default; duplicate IDs are invalid.
    pub fn receiver_dispatch_matches(
        &self,
        receiver: &Ty,
        implementations: &[ReceiverCase],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        if implementations.is_empty() || implementations.len() > 4096 {
            return Ok(false);
        }
        let budget = Budget::new(cancel);
        let mut expected = Vec::new();
        for implementation in &self.implementations {
            budget.step(0)?;
            let Some(pattern) = implementation.pattern() else {
                continue;
            };
            if Protocol::from_definition(&pattern.interface.declaration, &self.definitions)
                == Some(Protocol::PartialEq)
                && object_receiver_may_overlap(pattern.receiver, receiver)
            {
                let Some(case) = implementations
                    .iter()
                    .find(|case| case.implementation == *implementation.declaration())
                else {
                    return Ok(false);
                };
                if case.arguments.len() != pattern.parameters.len() {
                    return Ok(false);
                }
                for (path, parameter) in case.arguments.iter().zip(pattern.parameters) {
                    if path.len() >= 64 {
                        return Ok(false);
                    }
                    let mut target = pattern.receiver;
                    for step in path {
                        budget.step(0)?;
                        let Some(child) = receiver_child(target, step) else {
                            return Ok(false);
                        };
                        target = child;
                    }
                    if *target != parameter.as_type() {
                        return Ok(false);
                    }
                }
                expected.push(case.implementation);
            }
        }
        Ok(expected.len() == implementations.len()
            && expected.iter().all(|id| {
                implementations
                    .iter()
                    .filter(|actual| actual.implementation == *id)
                    .count()
                    == 1
            }))
    }

    /// Prove identity for every application of this receiver template. A more
    /// specific explicit header prevents this proof even if it does not match
    /// the currently symbolic arguments.
    pub fn receiver_identity_default(
        &self,
        receiver: &Ty,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);
        for implementation in &self.implementations {
            budget.step(0)?;
            let Some(pattern) = implementation.pattern() else {
                continue;
            };
            if Protocol::from_definition(&pattern.interface.declaration, &self.definitions)
                == Some(Protocol::PartialEq)
                && object_receiver_may_overlap(pattern.receiver, receiver)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// A failed guard permits the object's default only if no other explicit
    /// implementation shape could apply. Keep this proof independent of whether
    /// a particular caller currently satisfies the guarded bounds.
    pub fn receiver_comparison_conditions_match(
        &self,
        selected: &SharedMethodWitness,
        conditions: &[GenericBound],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);
        let mut found = false;
        for implementation in &self.implementations {
            budget.step(0)?;
            let Some(pattern) = implementation.pattern() else {
                continue;
            };
            if pattern.interface.declaration != selected.requirement.interface.declaration {
                continue;
            }
            if implementation.declaration() != &selected.implementation.declaration
                && object_receiver_may_overlap(pattern.receiver, &selected.requirement.receiver)
            {
                // Matching one symbolic header is insufficient to prove a
                // negative result: a more specific header could match later.
                return Ok(false);
            }
            let Some(bindings) = match_pattern(
                pattern,
                &selected.requirement.interface,
                &selected.requirement.receiver,
                cancel,
            )?
            else {
                continue;
            };
            if found || implementation.declaration() != &selected.implementation.declaration {
                return Ok(false);
            }
            let expected =
                bindings.apply_bounds(implementation.bounds(), &self.definitions, cancel)?;
            if expected.len() != conditions.len()
                || expected.iter().any(|bound| {
                    conditions
                        .iter()
                        .filter(|condition| *condition == bound)
                        .count()
                        != 1
                })
            {
                return Ok(false);
            }
            found = true;
        }
        Ok(found)
    }
}
