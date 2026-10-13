//! Intrinsic constraints of host schemas before linked implementation selection.
use crate::{
    host_interface::value_type::HostValueType,
    surface::StandardTypeConstraint,
    ty::substitution::{MAX_TYPE_DEPTH, MAX_TYPE_NODES},
};

/// Standard constraints of portable host schemas follow their script value shapes.
/// Concrete collections require their member protocols; interface views and opaque
/// host objects require separately declared implementations.
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
    let mut pending = vec![(ty, constraint, 1usize)];
    while let Some((ty, constraint, depth)) = pending.pop() {
        if remaining == 0 || depth > MAX_TYPE_DEPTH {
            return false;
        }
        remaining -= 1;
        match ty {
            HostValueType::Opaque(_) | HostValueType::List(..) => return false,
            HostValueType::Array(item) => {
                if constraint == StandardTypeConstraint::HashKey {
                    return false;
                }
                pending.push((item, constraint, depth + 1));
            }
            HostValueType::Vec(_, item) => pending.push((item, constraint, depth + 1)),
            HostValueType::Map { key, value, .. } => {
                pending.push((key, StandardTypeConstraint::HashKey, depth + 1));
                pending.push((value, constraint, depth + 1));
            }
            HostValueType::Set(item, _) => {
                pending.push((item, StandardTypeConstraint::HashKey, depth + 1))
            }
            HostValueType::F32 | HostValueType::F64
                if constraint == StandardTypeConstraint::HashKey =>
            {
                return false;
            }
            HostValueType::Tuple(items) => {
                if items.len() > remaining {
                    return false;
                }
                pending.extend(items.iter().map(|ty| (ty, constraint, depth + 1)));
            }
            HostValueType::Option(_, ty) => pending.push((ty, constraint, depth + 1)),
            HostValueType::Result { ok, error, .. } => pending.extend([
                (ok.as_ref(), constraint, depth + 1),
                (error.as_ref(), constraint, depth + 1),
            ]),
            HostValueType::Unit
            | HostValueType::Bool
            | HostValueType::I32
            | HostValueType::I64
            | HostValueType::F32
            | HostValueType::F64
            | HostValueType::String => {}
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::language::binding;
    use kagari_common::identity::map::DefinitionContext;

    #[test]
    fn host_standard_constraints_require_conditional_member_capabilities() {
        let context = DefinitionContext::new().unwrap();
        let float: HostValueType = HostValueType::F64;
        assert!(satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::Comparable
        ));
        assert!(!satisfies_standard_constraint(
            &float,
            StandardTypeConstraint::HashKey
        ));
        assert!(!satisfies_standard_constraint(
            &HostValueType::Option(
                context.intern(&binding::option_declaration()).unwrap(),
                Box::new(float.clone())
            ),
            StandardTypeConstraint::HashKey
        ));
        let array = HostValueType::Array(Box::new(float));
        assert!(!satisfies_standard_constraint(
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
