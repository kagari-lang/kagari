use super::*;
use crate::scalar::BuiltinType;
use kagari_common::identity::{
    DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};

fn owner(name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file("substitution.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn parameter(owner: &DefinitionId, position: usize) -> AbiType {
    AbiType::Parameter {
        owner: owner.clone(),
        position,
    }
}

#[test]
fn substitutes_one_binder_layer_without_capturing_caller_or_family_parameters() {
    let outer = owner("Outer");
    let family = associated_type_id(&outer, "Item");
    let first = parameter(&outer, 0);
    let second = parameter(&outer, 1);
    let local = parameter(&family, 0);
    let value = AbiType::Builtin(BuiltinType::I32);
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &second);
    substitution.bind(&outer, 1, &value);
    assert_eq!(
        substitution
            .apply(
                &AbiType::Tuple(vec![first, second.clone(), local.clone()]),
                &CancellationToken::default()
            )
            .unwrap(),
        AbiType::Tuple(vec![second, value, local]),
    );
}

#[test]
fn receiver_substitution_preserves_member_and_interface_binder_identity() {
    let outer = owner("Outer");
    let other = owner("Other");
    let value = AbiType::Builtin(BuiltinType::I32);
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&outer, &value);
    let input = AbiType::Tuple(vec![
        AbiType::SelfType(outer.clone()),
        AbiType::SelfType(other.clone()),
    ]);
    assert_eq!(
        substitution
            .apply(&input, &CancellationToken::default())
            .unwrap(),
        AbiType::Tuple(vec![value, AbiType::SelfType(other)])
    );
}

#[test]
fn substitution_rejects_oversized_replacement_expansion_and_honors_cancellation() {
    let outer = owner("Outer");
    let replacement = AbiType::Tuple(vec![AbiType::Builtin(BuiltinType::I32); MAX_TYPE_NODES / 2]);
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &replacement);
    let input = AbiType::Tuple(vec![parameter(&outer, 0), parameter(&outer, 0)]);
    assert_eq!(
        substitution.apply(&input, &CancellationToken::default()),
        Err(TypeTransformError::LimitExceeded)
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        substitution.apply(&parameter(&outer, 0), &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn substitution_checks_combined_depth_after_inserting_a_replacement() {
    let outer = owner("Outer");
    let mut replacement = AbiType::Builtin(BuiltinType::I32);
    for _ in 0..MAX_TYPE_DEPTH - 1 {
        replacement = AbiType::Iter(Box::new(replacement));
    }
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &replacement);
    assert!(
        substitution
            .apply(&parameter(&outer, 0), &CancellationToken::default())
            .is_ok()
    );
    assert_eq!(
        substitution.apply(
            &AbiType::Iter(Box::new(parameter(&outer, 0))),
            &CancellationToken::default()
        ),
        Err(TypeTransformError::LimitExceeded)
    );
}

fn projection(interface: &NominalAbiType, member: &DefinitionId) -> AbiType {
    AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(interface.declaration.clone())),
        interface: Box::new(interface.clone()),
        member: member.clone(),
        arguments: vec![],
    }
}

#[test]
fn associated_outputs_expand_nested_projections_but_preserve_unrelated_owners() {
    let outer = owner("Outer");
    let item = associated_type_id(&outer, "Item");
    let interface = NominalAbiType {
        declaration: outer,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let projected = projection(&interface, &item);
    let other = NominalAbiType {
        declaration: owner("Other"),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let unrelated = projection(&other, &associated_type_id(&other.declaration, "Item"));
    let applied = NominalAbiType {
        associated_types: [(item, AbiType::Builtin(BuiltinType::I32))].into(),
        ..interface
    };
    let input = AbiType::Tuple(vec![projected, unrelated.clone()]);
    assert_eq!(
        resolve_associated_outputs(&input, &applied, &CancellationToken::default()).unwrap(),
        AbiType::Tuple(vec![AbiType::Builtin(BuiltinType::I32), unrelated])
    );
}

#[test]
fn associated_output_cycles_fail_instead_of_recursing_indefinitely() {
    let outer = owner("Outer");
    let item = associated_type_id(&outer, "Item");
    let mut interface = NominalAbiType {
        declaration: outer,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let projected = projection(&interface, &item);
    interface.associated_types.insert(item, projected.clone());
    assert_eq!(
        resolve_associated_outputs(&projected, &interface, &CancellationToken::default()),
        Err(TypeTransformError::LimitExceeded)
    );
}
