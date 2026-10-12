//! Authenticate conditional receiver selection without treating it as a bound.
use crate::{
    callable::{receiver::object_receiver_may_overlap, witness::SharedMethodWitness},
    types::proofs::{Budget, ProofCatalog},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    language::Protocol,
    ty::{GenericBound, Ty, matching::match_pattern, substitution::TypeTransformError},
};

impl ProofCatalog<'_> {
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
