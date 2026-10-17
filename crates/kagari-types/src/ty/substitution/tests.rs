use super::*;
use crate::scalar::BuiltinType;
use kagari_common::identity::{DefinitionKind, ModuleIdentity, map::DefinitionContext};

fn owner(context: &DefinitionContext, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file("substitution.kgr"))
        .unwrap();
    context
        .intern_child(root, DefinitionKind::Trait, name, 0)
        .unwrap()
}

fn associated(context: &DefinitionContext, owner: DefinitionId, name: &str) -> DefinitionId {
    context
        .intern_child(owner, DefinitionKind::AssociatedType, name, 0)
        .unwrap()
}

fn parameter(owner: &DefinitionId, position: usize) -> Ty {
    Ty::Parameter {
        owner: *owner,
        position,
    }
}

#[test]
fn substitutes_one_binder_layer_without_capturing_caller_or_family_parameters() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let family = associated(&context, outer, "Item");
    let first = parameter(&outer, 0);
    let second = parameter(&outer, 1);
    let local = parameter(&family, 0);
    let value = Ty::Builtin(BuiltinType::I32);
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &second);
    substitution.bind(&outer, 1, &value);
    assert_eq!(
        substitution
            .apply(
                &Ty::Tuple(vec![first, second.clone(), local.clone()]),
                &CancellationToken::default()
            )
            .unwrap(),
        Ty::Tuple(vec![second, value, local]),
    );
}

#[test]
fn receiver_substitution_preserves_member_and_interface_binder_identity() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let other = owner(&context, "Other");
    let value = Ty::Builtin(BuiltinType::I32);
    let mut substitution = TypeSubstitution::default();
    substitution.bind_receiver(&outer, &value);
    let input = Ty::Tuple(vec![Ty::SelfType(outer), Ty::SelfType(other)]);
    assert_eq!(
        substitution
            .apply(&input, &CancellationToken::default())
            .unwrap(),
        Ty::Tuple(vec![value, Ty::SelfType(other)])
    );
}

#[test]
fn substitution_rejects_oversized_replacement_expansion_and_honors_cancellation() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let replacement = Ty::Tuple(vec![Ty::Builtin(BuiltinType::I32); MAX_TYPE_NODES / 2]);
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &replacement);
    let input = Ty::Tuple(vec![parameter(&outer, 0), parameter(&outer, 0)]);
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
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let mut replacement = Ty::Builtin(BuiltinType::I32);
    for _ in 0..MAX_TYPE_DEPTH - 1 {
        replacement = Ty::Iter(Box::new(replacement));
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
            &Ty::Iter(Box::new(parameter(&outer, 0))),
            &CancellationToken::default()
        ),
        Err(TypeTransformError::LimitExceeded)
    );
}

fn projection(interface: &NominalTy, member: &DefinitionId) -> Ty {
    Ty::Projection {
        receiver: Box::new(Ty::SelfType(interface.declaration)),
        interface: Box::new(interface.clone()),
        member: *member,
        arguments: vec![],
    }
}

#[test]
fn associated_outputs_expand_nested_projections_but_preserve_unrelated_owners() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let item = associated(&context, outer, "Item");
    let interface = NominalTy {
        declaration: outer,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let projected = projection(&interface, &item);
    let other = NominalTy {
        declaration: owner(&context, "Other"),
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let unrelated = projection(&other, &associated(&context, other.declaration, "Item"));
    let applied = NominalTy {
        associated_types: [(item, Ty::Builtin(BuiltinType::I32))].into(),
        ..interface
    };
    let mut other_receiver = projected.clone();
    let Ty::Projection { receiver, .. } = &mut other_receiver else {
        unreachable!()
    };
    **receiver = Ty::Parameter {
        owner: owner(&context, "source"),
        position: 0,
    };
    let input = Ty::Tuple(vec![projected, unrelated.clone(), other_receiver.clone()]);
    assert_eq!(
        resolve_associated_outputs(
            &input,
            &applied,
            &Ty::SelfType(applied.declaration),
            &CancellationToken::default()
        )
        .unwrap(),
        Ty::Tuple(vec![
            Ty::Builtin(BuiltinType::I32),
            unrelated,
            other_receiver
        ])
    );
}

#[test]
fn associated_output_cycles_fail_instead_of_recursing_indefinitely() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let item = associated(&context, outer, "Item");
    let mut interface = NominalTy {
        declaration: outer,
        arguments: vec![],
        associated_types: BTreeMap::new(),
    };
    let projected = projection(&interface, &item);
    interface.associated_types.insert(item, projected.clone());
    assert_eq!(
        resolve_associated_outputs(
            &projected,
            &interface,
            &Ty::SelfType(interface.declaration),
            &CancellationToken::default()
        ),
        Err(TypeTransformError::LimitExceeded)
    );
}

#[test]
fn applied_bounds_merge_equal_receivers_and_keep_canonical_order() {
    let context = DefinitionContext::new().unwrap();
    let outer = owner(&context, "Outer");
    let target = Ty::Builtin(BuiltinType::I32);
    // Allocate the later spelling first so ID order differs from canonical order.
    let second = NominalTy {
        declaration: owner(&context, "Second"),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let first = NominalTy {
        declaration: owner(&context, "First"),
        arguments: vec![],
        associated_types: Default::default(),
    };
    let mut substitution = TypeSubstitution::default();
    substitution.bind(&outer, 0, &target);
    substitution.bind(&outer, 1, &target);
    let actual = substitution
        .apply_bounds(
            &[
                GenericBound {
                    ty: parameter(&outer, 0),
                    constraints: vec![second.clone()],
                },
                GenericBound {
                    ty: parameter(&outer, 1),
                    constraints: vec![first.clone(), second.clone()],
                },
            ],
            &context.snapshot(),
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(
        actual,
        vec![GenericBound {
            ty: target,
            constraints: vec![first, second]
        }]
    );
}
