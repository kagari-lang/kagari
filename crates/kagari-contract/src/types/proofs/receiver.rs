//! Authenticate conditional receiver selection without treating it as a bound.
use crate::{
    callable::witness::SharedMethodWitness,
    types::proofs::{Budget, ProofCatalog},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::ty::{
    GenericBound, Ty, matching::match_pattern, substitution::TypeTransformError,
};

impl ProofCatalog<'_> {
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
                && may_share_receiver(pattern.receiver, &selected.requirement.receiver)
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

/// Conservative constructor overlap for a conditional comparison certificate.
/// Rejection does not assert that an implementation is inapplicable.
fn may_share_receiver(left: &Ty, right: &Ty) -> bool {
    match (left, right) {
        (Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Projection { .. }, _)
        | (_, Ty::Parameter { .. } | Ty::SelfType(_) | Ty::Projection { .. }) => true,
        (Ty::Struct(left), Ty::Struct(right))
        | (Ty::NativeObject(left), Ty::NativeObject(right)) => {
            left.declaration == right.declaration
        }
        (Ty::Array(_), Ty::Array(_)) => true,
        (Ty::Map { access: left, .. }, Ty::Map { access: right, .. })
        | (Ty::Set(_, left), Ty::Set(_, right)) => left == right,
        _ => false,
    }
}
