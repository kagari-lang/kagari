//! Collection capability and value-copy checks over concrete executable types.
use crate::types::substitution::{MAX_TYPE_NODES, TypeSubstitution};
use crate::types::{AbiType, NominalAbiType};
use kagari_common::cancellation::CancellationToken;
use kagari_common::collection::CollectionAccess;
use std::collections::HashSet;

impl AbiType {
    pub fn collection_access(&self) -> Option<CollectionAccess> {
        match self {
            Self::Array(_, access) | Self::Set(_, access) | Self::Map { access, .. } => {
                Some(*access)
            }
            _ => None,
        }
    }

    /// Weaken only the outer storage capability; element types remain invariant.
    pub fn read_only_view(&self) -> Option<Self> {
        Some(match self {
            Self::Array(item, _) => Self::Array(item.clone(), CollectionAccess::ReadOnly),
            Self::Set(item, _) => Self::Set(item.clone(), CollectionAccess::ReadOnly),
            Self::Map { key, value, .. } => Self::Map {
                key: key.clone(),
                value: value.clone(),
                access: CollectionAccess::ReadOnly,
            },
            _ => return None,
        })
    }

    pub fn can_weaken_to(&self, target: &Self) -> bool {
        if self.collection_access() != Some(CollectionAccess::Mutable)
            || target.collection_access() != Some(CollectionAccess::ReadOnly)
        {
            return false;
        }
        match (self, target) {
            (Self::Array(a, _), Self::Array(b, _)) | (Self::Set(a, _), Self::Set(b, _)) => a == b,
            (
                Self::Map {
                    key: ak, value: av, ..
                },
                Self::Map {
                    key: bk, value: bv, ..
                },
            ) => ak == bk && av == bv,
            _ => false,
        }
    }
}

/// Repetition copies value-like elements but never duplicates a shared object.
/// Enum payloads come from validated layouts in the current executable closure.
pub fn supports_array_repetition(
    ty: &AbiType,
    mut enum_payload: impl FnMut(&NominalAbiType) -> Option<Vec<AbiType>>,
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
            AbiType::Builtin(_) | AbiType::Range(_, _) => {}
            AbiType::Tuple(items) | AbiType::StandardEnum { args: items, .. } => {
                pending.extend(items)
            }
            AbiType::Enum(instance) => {
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
    fn weakening_is_outer_only_and_does_not_change_storage_families() {
        let item = AbiType::Builtin(BuiltinType::I32);
        let mutable = AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable);
        let readonly = mutable.read_only_view().unwrap();
        assert!(mutable.can_weaken_to(&readonly));
        assert!(!readonly.can_weaken_to(&mutable));
        assert!(!mutable.can_weaken_to(&AbiType::Set(Box::new(item), CollectionAccess::ReadOnly)));
        let nested_mutable = AbiType::Array(Box::new(mutable), CollectionAccess::Mutable);
        let nested_readonly = AbiType::Array(Box::new(readonly), CollectionAccess::ReadOnly);
        assert!(!nested_mutable.can_weaken_to(&nested_readonly));
    }

    #[test]
    fn repetition_rejects_shared_payloads_and_checks_cancellation() {
        let scalar = AbiType::Builtin(BuiltinType::I32);
        let shared = AbiType::Array(Box::new(scalar.clone()), CollectionAccess::ReadOnly);
        let cancel = CancellationToken::default();
        assert!(supports_array_repetition(
            &AbiType::Tuple(vec![scalar.clone()]),
            |_| None,
            &cancel
        ));
        assert!(!supports_array_repetition(
            &AbiType::Tuple(vec![shared]),
            |_| None,
            &cancel
        ));
        cancel.cancel();
        assert!(!supports_array_repetition(&scalar, |_| None, &cancel));
    }
}
