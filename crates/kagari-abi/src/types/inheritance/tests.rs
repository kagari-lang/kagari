use super::*;
use crate::{
    scalar::BuiltinType,
    standard::traits::{self, StandardTrait},
    types::GenericParameterAbi,
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};
use std::collections::BTreeMap;

fn id(name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file("inheritance.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}
fn applied(owner: &DefinitionId, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: owner.clone(),
        arguments,
        associated_types: BTreeMap::new(),
    }
}
fn contract(owner: &DefinitionId, arity: usize, parents: Vec<NominalAbiType>) -> TraitAbi {
    TraitAbi {
        name: owner.path.last().unwrap().name.clone(),
        generic_params: (0..arity)
            .map(|position| GenericParameterAbi {
                owner: owner.clone(),
                position,
            })
            .collect(),
        bounds: vec![],
        methods: vec![],
        default_methods: vec![],
        associated_consts: vec![],
        associated_types: vec![],
        supertraits: parents,
    }
}

#[test]
fn applied_ancestry_substitutes_outputs_parameters_and_receiver_before_deduplicating() {
    let derived = id("Derived");
    let left = id("Left");
    let right = id("Right");
    let base = id("Base");
    let item = associated_type_id(&derived, "Item");
    let parameter = AbiType::Parameter {
        owner: derived.clone(),
        position: 0,
    };
    let mut root = applied(&derived, vec![AbiType::Builtin(BuiltinType::I32)]);
    root.associated_types
        .insert(item.clone(), AbiType::Builtin(BuiltinType::Bool));
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::SelfType(derived.clone())),
        interface: Box::new(applied(&derived, vec![parameter.clone()])),
        member: item,
        arguments: vec![],
    };
    let receiver = AbiType::Builtin(BuiltinType::String);
    let input = AbiType::Tuple(vec![
        parameter,
        projection,
        AbiType::SelfType(derived.clone()),
    ]);
    let records: BTreeMap<_, _> = [
        (
            derived.clone(),
            contract(
                &derived,
                1,
                vec![
                    applied(&left, vec![input.clone()]),
                    applied(&right, vec![input]),
                ],
            ),
        ),
        (
            left.clone(),
            contract(
                &left,
                1,
                vec![applied(
                    &base,
                    vec![AbiType::Parameter {
                        owner: left.clone(),
                        position: 0,
                    }],
                )],
            ),
        ),
        (
            right.clone(),
            contract(
                &right,
                1,
                vec![applied(
                    &base,
                    vec![AbiType::Parameter {
                        owner: right.clone(),
                        position: 0,
                    }],
                )],
            ),
        ),
        (base.clone(), contract(&base, 1, vec![])),
    ]
    .into();
    let closure = trait_closure(&root, &receiver, &CancellationToken::default(), &|id| {
        records.get(id)
    })
    .unwrap();
    assert_eq!(closure.len(), 4);
    assert_eq!(
        closure[2],
        applied(
            &base,
            vec![AbiType::Tuple(vec![
                AbiType::Builtin(BuiltinType::I32),
                AbiType::Builtin(BuiltinType::Bool),
                receiver
            ])]
        )
    );
}

#[test]
fn ancestry_rejects_expanding_cycles_missing_contracts_and_wrong_arity() {
    let recursive = id("Recursive");
    let root = applied(&recursive, vec![AbiType::Builtin(BuiltinType::I32)]);
    let record = contract(
        &recursive,
        1,
        vec![applied(
            &recursive,
            vec![AbiType::Iter(Box::new(AbiType::Parameter {
                owner: recursive.clone(),
                position: 0,
            }))],
        )],
    );
    let receiver = AbiType::SelfType(recursive.clone());
    assert_eq!(
        trait_closure(&root, &receiver, &CancellationToken::default(), &|_| Some(
            &record
        )),
        Err(TypeTransformError::LimitExceeded)
    );
    assert_eq!(
        trait_closure(&root, &receiver, &CancellationToken::default(), &|_| None),
        Err(TypeTransformError::InvalidContract)
    );
    assert_eq!(
        trait_closure(
            &applied(&recursive, vec![]),
            &receiver,
            &CancellationToken::default(),
            &|_| Some(&record)
        ),
        Err(TypeTransformError::InvalidContract)
    );
}

#[test]
fn generated_standard_ancestry_closes_without_source_signatures_and_cancels() {
    for kind in StandardTrait::ALL {
        let owner = traits::identity(kind);
        let record = standard_trait_contract(&owner).unwrap();
        let root = applied(
            &owner,
            record
                .generic_params
                .iter()
                .map(GenericParameterAbi::as_type)
                .collect(),
        );
        assert!(
            trait_closure(
                &root,
                &AbiType::SelfType(owner.clone()),
                &CancellationToken::default(),
                &|_| None
            )
            .is_ok(),
            "{}",
            kind.name()
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert_eq!(
            trait_closure(&root, &AbiType::SelfType(owner), &cancel, &|_| None),
            Err(TypeTransformError::Cancelled)
        );
    }
}
