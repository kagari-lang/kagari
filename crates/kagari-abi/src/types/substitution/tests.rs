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

#[test]
fn impl_instantiation_preserves_method_generics_and_substitutes_their_bounds() {
    use crate::{
        callable::CallableImplementation,
        types::{FunctionAbi, InterfaceTableAbi, ParameterAbi},
    };
    let implementation = owner("Impl");
    let method = associated_type_id(&implementation, "construct");
    let interface = owner("Iterable");
    let item = associated_type_id(&interface, "Item");
    let outer = GenericParameterAbi {
        owner: implementation.clone(),
        position: 0,
    };
    let local = GenericParameterAbi {
        owner: method.clone(),
        position: 0,
    };
    let source = parameter(&method, 0);
    let input = parameter(&implementation, 0);
    let table = InterfaceTableAbi {
        associated_type_families: vec![],
        associated_consts: vec![],
        host_bridge: false,
        declaration: implementation.clone(),
        name: "Impl".into(),
        generic_params: vec![outer.clone()],
        bounds: vec![],
        trait_type: AbiType::Trait(NominalAbiType {
            declaration: interface.clone(),
            arguments: vec![],
            associated_types: BTreeMap::new(),
        }),
        for_type: AbiType::Tuple(vec![input.clone()]),
        methods: vec![FunctionAbi {
            method_policy: Default::default(),
            name: "construct".into(),
            implementation: CallableImplementation::Script,
            generic_params: vec![local.clone()],
            bounds: vec![GenericBoundAbi {
                ty: source.clone(),
                constraints: vec![ConstraintAbi::Trait(NominalAbiType {
                    declaration: interface,
                    arguments: vec![],
                    associated_types: BTreeMap::from([(item.clone(), input.clone())]),
                })],
            }],
            params: vec![ParameterAbi {
                name: "source".into(),
                mutable: false,
                ty: source.clone(),
            }],
            return_type: AbiType::Tuple(vec![input]),
        }],
    };
    let number = AbiType::Builtin(BuiltinType::I32);
    let applied = table.instantiate(std::slice::from_ref(&number)).unwrap();
    assert_eq!(applied.for_type, AbiType::Tuple(vec![number.clone()]));
    assert!(applied.generic_params.is_empty());
    let method = &applied.methods[0];
    assert_eq!(method.generic_params, [local]);
    assert_eq!(method.params[0].ty, source);
    assert_eq!(method.return_type, AbiType::Tuple(vec![number.clone()]));
    assert_eq!(method.bounds[0].ty, source);
    let ConstraintAbi::Trait(bound) = &method.bounds[0].constraints[0] else {
        panic!("Iterable bound")
    };
    assert_eq!(bound.associated_types[&item], number);
}

#[test]
fn applied_bounds_merge_equal_receivers_and_keep_canonical_order() {
    let outer = owner("Outer");
    let target = AbiType::Builtin(BuiltinType::I32);
    let first = ConstraintAbi::Trait(NominalAbiType {
        declaration: owner("First"),
        arguments: vec![],
        associated_types: Default::default(),
    });
    let second = ConstraintAbi::Trait(NominalAbiType {
        declaration: owner("Second"),
        arguments: vec![],
        associated_types: Default::default(),
    });
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &target);
    substitution.bind(&outer, 1, &target);
    let actual = substitution
        .apply_bounds(
            &[
                GenericBoundAbi {
                    ty: parameter(&outer, 0),
                    constraints: vec![second.clone()],
                },
                GenericBoundAbi {
                    ty: parameter(&outer, 1),
                    constraints: vec![first.clone(), second.clone()],
                },
            ],
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        actual,
        vec![GenericBoundAbi {
            ty: target,
            constraints: vec![first, second]
        }]
    );
}
