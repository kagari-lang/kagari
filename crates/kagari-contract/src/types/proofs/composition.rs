//! Bounded classification of core equality composition from executable facts.
use crate::{
    language::primitive as intrinsic,
    types::proofs::{Budget, ProofCatalog, search::Search},
};
use kagari_abi::representation::ValueType;
use kagari_common::cancellation::CancellationToken;
use kagari_types::{
    language::Protocol,
    ty::{Ty, substitution::TypeTransformError},
};
use std::collections::HashSet;

impl ProofCatalog<'_> {
    /// Validate primitive equality/hash without granting a fallback or bypassing
    /// a selected callback. Aggregate physical slots require exact type evidence.
    pub fn primitive_comparison_valid(
        &self,
        protocol: Protocol,
        arguments: &[(ValueType, Option<&Ty>)],
        cancel: &CancellationToken,
    ) -> Result<bool, TypeTransformError> {
        let count = if protocol == Protocol::PartialEq {
            2
        } else if protocol == Protocol::Hash {
            1
        } else {
            return Ok(false);
        };
        if arguments.len() != count {
            return Ok(false);
        }
        let (physical, semantic) = arguments[0];
        if arguments.iter().any(|(ty, _)| *ty != physical) {
            return Ok(false);
        }
        match physical {
            ValueType::Unit
            | ValueType::Bool
            | ValueType::I32
            | ValueType::I64
            | ValueType::U64
            | ValueType::Str
            | ValueType::Char => Ok(true),
            ValueType::F32 | ValueType::F64 => Ok(protocol == Protocol::PartialEq),
            ValueType::HeapObject => {
                let Some(receiver) = semantic else {
                    return Ok(false);
                };
                if arguments.iter().any(|(_, ty)| *ty != Some(receiver)) {
                    return Ok(false);
                }
                Ok(self.holds(
                    &intrinsic::applied(protocol, vec![], &self.definitions)?,
                    receiver,
                    &[],
                    cancel,
                )? && !self.uses_custom_equality(receiver, cancel)?)
            }
            _ => Ok(false),
        }
    }

    /// Whether primitive member equality would bypass a selected
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
                Ty::Array(_)
                | Ty::NativeObject(_)
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
