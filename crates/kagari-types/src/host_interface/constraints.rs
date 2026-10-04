//! Intrinsic constraints of host schemas before linked implementation selection.
use crate::{
    host_interface::value_type::HostValueType,
    surface::StandardTypeConstraint,
    ty::substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
};
/// Intrinsic standard constraints of portable host values. Collections use shared
/// identity; tuple/Option/Result equality and hashing recurse into their payloads.
/// Nominal host objects need declared trait implementations, not this fallback.
pub fn satisfies_standard_constraint(
    ty: &HostValueType,
    constraint: StandardTypeConstraint,
) -> bool {
    if matches!(
        constraint,
        StandardTypeConstraint::OrderedNumber | StandardTypeConstraint::SignedNumber
    ) {
        return matches!(
            ty,
            HostValueType::I32 | HostValueType::I64 | HostValueType::F32 | HostValueType::F64
        );
    }
    let mut remaining = MAX_TYPE_NODES;
    let mut pending = vec![(ty, 1usize)];
    while let Some((ty, depth)) = pending.pop() {
        if remaining == 0 || depth > MAX_TYPE_DEPTH {
            return false;
        }
        remaining -= 1;
        match ty {
            HostValueType::Opaque(_) => return false,
            HostValueType::F32 | HostValueType::F64
                if constraint == StandardTypeConstraint::HashKey =>
            {
                return false;
            }
            HostValueType::Tuple(items) => {
                if items.len() > remaining {
                    return false;
                }
                pending.extend(items.iter().map(|ty| (ty, depth + 1)));
            }
            HostValueType::Option(ty) => pending.push((ty, depth + 1)),
            HostValueType::Result { ok, error } => {
                pending.extend([(ok.as_ref(), depth + 1), (error.as_ref(), depth + 1)])
            }
            HostValueType::Unit
            | HostValueType::Bool
            | HostValueType::I32
            | HostValueType::I64
            | HostValueType::F32
            | HostValueType::F64
            | HostValueType::String
            | HostValueType::Array(_, _)
            | HostValueType::Map { .. }
            | HostValueType::Set(_, _) => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection::CollectionAccess;

    #[test]
    fn host_standard_constraints_distinguish_payload_and_collection_identity() {
        let float = HostValueType::F64;
        assert!(satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::Comparable
        ));
        assert!(!satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::HashKey
        ));
        assert!(!satisfies_standard_constraint(
            &HostValueType::Option(Box::new(float.clone())),
            StandardTypeConstraint::HashKey
        ));
        let array = HostValueType::Array(Box::new(float), CollectionAccess::ReadOnly);
        assert!(satisfies_standard_constraint(
            &array,
            StandardTypeConstraint::HashKey
        ));
        assert!(satisfies_standard_constraint(
            &HostValueType::Tuple(vec![array, HostValueType::I32]),
            StandardTypeConstraint::Comparable
        ));
        assert!(!satisfies_standard_constraint(
            &HostValueType::Bool,
            StandardTypeConstraint::SignedNumber
        ));
        assert!(satisfies_standard_constraint(
            &HostValueType::F32,
            StandardTypeConstraint::SignedNumber
        ));
    }
}
