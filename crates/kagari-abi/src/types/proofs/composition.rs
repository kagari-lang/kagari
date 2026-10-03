//! Bounded classification of core equality composition from executable facts.
use crate::{
    language::{Protocol, primitive as intrinsic},
    types::{
        AbiType,
        proofs::{Budget, ProofCatalog, search::Search},
        substitution::TypeTransformError,
    },
};
use kagari_common::cancellation::CancellationToken;
use std::collections::HashSet;

impl ProofCatalog<'_> {
    /// Whether native identity/structural equality would bypass a selected
    /// nominal PartialEq implementation in this value's tuple or enum payloads.
    pub fn uses_custom_equality(
        &self,
        receiver: &AbiType,
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
                AbiType::NativeObject(_) | AbiType::Struct(_) | AbiType::Enum(_) => {
                    if self.explicit(
                        &intrinsic::applied(Protocol::PartialEq, vec![]),
                        ty,
                        &[],
                        &mut search,
                        &budget,
                        depth,
                    )? != 0
                    {
                        return Ok(true);
                    }
                    if let AbiType::Enum(instance) = ty {
                        let Some(members) = self.enumerations.get(instance) else {
                            return Err(TypeTransformError::InvalidContract);
                        };
                        pending.extend(members.iter().map(|member| (*member, depth + 1)));
                    }
                }
                AbiType::Tuple(members) | AbiType::StandardEnum { args: members, .. } => {
                    pending.extend(members.iter().map(|member| (member, depth + 1)))
                }
                _ => {}
            }
        }
        Ok(false)
    }
}
