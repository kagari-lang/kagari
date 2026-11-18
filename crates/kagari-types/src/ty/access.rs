//! Value-copy checks over semantic types.
use crate::ty::{
    NominalTy, Ty,
    substitution::{MAX_TYPE_NODES, TypeSubstitution},
};

use kagari_common::cancellation::CancellationToken;
use std::collections::HashSet;

/// Repetition copies value-like elements but never duplicates a shared object.
/// The caller supplies the checked payloads of each applied enum.
pub fn supports_array_repetition(
    ty: &Ty,
    mut enum_payload: impl FnMut(&NominalTy) -> Option<Vec<Ty>>,
    cancel: &CancellationToken,
) -> bool {
    let copier = TypeSubstitution::default();
    let Ok(root) = copier.apply(ty, cancel) else {
        return false;
    };
    let mut pending = vec![root];
    let mut seen = HashSet::new();
    let mut remaining = MAX_TYPE_NODES;
    while let Some(ty) = pending.pop() {
        if cancel.check().is_err() || remaining == 0 {
            return false;
        }
        remaining -= 1;
        if !seen.insert(ty.clone()) {
            continue;
        }
        match ty {
            Ty::Builtin(_) | Ty::Range(_, _) => {}
            Ty::Tuple(items) => pending.extend(items),
            Ty::Enum(instance) => {
                let Some(payload) = enum_payload(&instance) else {
                    return false;
                };
                if payload.len() > remaining {
                    return false;
                }
                for ty in payload {
                    let Ok(ty) = copier.apply(&ty, cancel) else {
                        return false;
                    };
                    pending.push(ty);
                }
            }
            _ => return false,
        }
        if pending.len() > remaining {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scalar::BuiltinType;

    #[test]
    fn repetition_rejects_shared_payloads_and_checks_cancellation() {
        let scalar = Ty::Builtin(BuiltinType::I32);
        let shared = Ty::Array(Box::new(scalar.clone()));
        let cancel = CancellationToken::default();
        assert!(supports_array_repetition(
            &Ty::Tuple(vec![scalar.clone()]),
            |_| None,
            &cancel
        ));
        assert!(!supports_array_repetition(
            &Ty::Tuple(vec![shared]),
            |_| None,
            &cancel
        ));
        cancel.cancel();
        assert!(!supports_array_repetition(&scalar, |_| None, &cancel));
    }
}
