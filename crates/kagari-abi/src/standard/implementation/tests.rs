use super::*;
use crate::scalar::BuiltinType;
use crate::standard::intrinsic;
use crate::standard::surface::STANDARD_IMPLEMENTATIONS;
use kagari_common::collection::CollectionAccess;

#[test]
fn generated_applications_retain_outputs_and_all_declared_requirements() {
    let cancel = CancellationToken::default();
    for declaration in STANDARD_IMPLEMENTATIONS {
        let mut bindings = StandardArguments::new(declaration.generics);
        for name in declaration.generics {
            bindings
                .bind(
                    &crate::standard::declarations::ApiType::Named(name, &[]),
                    &AbiType::Builtin(BuiltinType::I32),
                    &cancel,
                )
                .unwrap();
        }
        let receiver = bindings
            .resolve(&declaration.target, &cancel)
            .unwrap()
            .unwrap();
        bindings.bind_receiver(&receiver, &cancel).unwrap();
        let contract = applied_contract(declaration, &bindings, &cancel).unwrap();
        let matched = match_application(declaration, &contract, &receiver, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(
            applied_contract(declaration, &matched, &cancel).unwrap(),
            contract
        );
        assert_eq!(
            requirements(declaration, &matched, &cancel).unwrap().len(),
            declaration.bounds.len()
        );
        for (member, output) in &contract.associated_types {
            let mut wrong = contract.clone();
            wrong
                .associated_types
                .insert(member.clone(), AbiType::Tuple(vec![output.clone()]));
            assert!(
                match_application(declaration, &wrong, &receiver, &cancel)
                    .unwrap()
                    .is_none()
            );
        }
    }
}

#[test]
fn readonly_storage_only_matches_readonly_capabilities_and_invariant_arguments() {
    let cancel = CancellationToken::default();
    let item = AbiType::Builtin(BuiltinType::I32);
    let receiver = AbiType::Array(Box::new(item.clone()), CollectionAccess::ReadOnly);
    for kind in [StandardTrait::List, StandardTrait::MutableList] {
        let declaration = STANDARD_IMPLEMENTATIONS
            .iter()
            .find(|d| d.interface == kind.name())
            .unwrap();
        assert_eq!(
            match_application(
                declaration,
                &intrinsic::applied(kind, vec![item.clone()]),
                &receiver,
                &cancel
            )
            .unwrap()
            .is_some(),
            kind == StandardTrait::List
        );
        assert!(
            match_application(
                declaration,
                &intrinsic::applied(kind, vec![AbiType::Builtin(BuiltinType::Bool)]),
                &receiver,
                &cancel
            )
            .unwrap()
            .is_none()
        );
    }
    cancel.cancel();
    assert!(matches!(
        match_receiver(&STANDARD_IMPLEMENTATIONS[0], &receiver, &cancel),
        Err(TypeTransformError::Cancelled)
    ));
}

#[test]
fn trait_only_parameters_bind_without_changing_receiver_and_repeated_inputs_must_agree() {
    use crate::standard::surface::StandardEnum;
    use kagari_common::range::RangeKind;
    let cancel = CancellationToken::default();
    let declaration = STANDARD_IMPLEMENTATIONS
        .iter()
        .find(|d| {
            d.interface == "RangeBounds"
                && matches!(
                    d.target,
                    crate::standard::declarations::ApiType::Named("RangeFull", [])
                )
        })
        .unwrap();
    let range = AbiType::Range(
        Box::new(AbiType::Builtin(BuiltinType::Unit)),
        RangeKind::Full,
    );
    let required = intrinsic::applied(
        StandardTrait::RangeBounds,
        vec![AbiType::Builtin(BuiltinType::U64)],
    );
    assert!(
        match_application(declaration, &required, &range, &cancel)
            .unwrap()
            .is_some()
    );
    let declaration = STANDARD_IMPLEMENTATIONS
        .iter()
        .find(|d| {
            d.interface == "FromIterator"
                && matches!(
                    d.target,
                    crate::standard::declarations::ApiType::Named("Result", _)
                )
        })
        .unwrap();
    let item = AbiType::Builtin(BuiltinType::I32);
    let error = AbiType::Builtin(BuiltinType::String);
    let receiver = AbiType::StandardEnum {
        kind: StandardEnum::Result,
        args: vec![
            AbiType::Array(Box::new(item.clone()), CollectionAccess::Mutable),
            error.clone(),
        ],
    };
    let mut required = intrinsic::applied(
        StandardTrait::FromIterator,
        vec![AbiType::StandardEnum {
            kind: StandardEnum::Result,
            args: vec![item.clone(), error],
        }],
    );
    assert!(
        match_application(declaration, &required, &receiver, &cancel)
            .unwrap()
            .is_some()
    );
    required.arguments[0] = AbiType::StandardEnum {
        kind: StandardEnum::Result,
        args: vec![item, AbiType::Builtin(BuiltinType::Bool)],
    };
    assert!(
        match_application(declaration, &required, &receiver, &cancel)
            .unwrap()
            .is_none()
    );
}
