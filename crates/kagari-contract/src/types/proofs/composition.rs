//! Bounded classification of core equality composition from executable facts.
use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, search::Search},
};
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    language::Protocol,
    ty::{Ty, substitution::TypeTransformError},
};
use std::collections::HashSet;

impl ProofCatalog<'_> {
    /// Whether native identity/structural equality would bypass a selected
    /// nominal PartialEq implementation in this value's tuple or enum payloads.
    pub fn uses_custom_equality(
        &self,
        receiver: &Ty,
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let budget = Budget::new(cancel);
        let mut search = Search::default();
        let mut pending = vec![(receiver, 0)];
        let mut seen = HashSet::new();
        while let Some((ty, depth)) = pending.pop() {
            budget.step(depth)?;
            if !seen.insert(ty) {
                continue;
            }
            match ty {
                Ty::Trait(_) => return Ok(true),
                Ty::NativeObject(_)
                | Ty::Struct(_)
                | Ty::Enum(_)
                | Ty::Map { .. }
                | Ty::Set(_, _) => {
                    if self.explicit(
                        &intrinsic::applied(Protocol::PartialEq, vec![], &self.definitions)?,
                        ty,
                        &[],
                        &mut search,
                        &budget,
                        depth,
                    )? != 0
                    {
                        return Ok(true);
                    }
                    if let Ty::Enum(instance) = ty {
                        let Some(members) = self.enumerations.get(instance) else {
                            return Err(TypeTransformError::InvalidContract);
                        };
                        pending.extend(members.iter().map(|member| (*member, depth + 1)));
                    }
                }
                Ty::Tuple(members) => {
                    pending.extend(members.iter().map(|member| (member, depth + 1)))
                }
                _ => {}
            }
        }
        Ok(false)
    }
}
