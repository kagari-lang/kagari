use super::*;
use crate::{
    language::{self, Protocol},
    scalar::BuiltinType,
    types::{GenericParam, PublicItem, TraitContract, trait_contract},
};
use kagari_common::identity::{
    DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id,
};
use std::collections::BTreeMap;

fn id(name: &str) -> DefinitionPath {
    DefinitionPath {
        module: ModuleIdentity::single_file("inheritance.kgr"),
        path: vec![DefinitionPathSegment {
            kind: DefinitionKind::Trait,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn applied(owner: &DefinitionPath, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: owner.clone(),
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn contract(owner: &DefinitionPath, arity: usize, parents: Vec<NominalTy>) -> TraitDef {
    TraitDef {
        conversion_adapter: None,
        storage_access: None,
        name: owner.path.last().unwrap().name.clone(),
        generic_params: (0..arity)
            .map(|position| GenericParam {
                owner: owner.clone(),
                position,
            })
            .collect(),
        bounds: vec![],
        methods: vec![],
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
    let parameter = Ty::Parameter {
        owner: derived.clone(),
        position: 0,
    };
    let mut root = applied(&derived, vec![Ty::Builtin(BuiltinType::I32)]);
    root.associated_types
        .insert(item.clone(), Ty::Builtin(BuiltinType::Bool));
    let projection = Ty::Projection {
        receiver: Box::new(Ty::SelfType(derived.clone())),
        interface: Box::new(applied(&derived, vec![parameter.clone()])),
        member: item,
        arguments: vec![],
    };
    let receiver = Ty::Builtin(BuiltinType::String);
    let input = Ty::Tuple(vec![parameter, projection, Ty::SelfType(derived.clone())]);
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
                    vec![Ty::Parameter {
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
                    vec![Ty::Parameter {
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
            vec![Ty::Tuple(vec![
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::Bool),
                receiver
            ])]
        )
    );
}

#[test]
fn ancestry_rejects_expanding_cycles_missing_contracts_and_wrong_arity() {
    let recursive = id("Recursive");
    let root = applied(&recursive, vec![Ty::Builtin(BuiltinType::I32)]);
    let record = contract(
        &recursive,
        1,
        vec![applied(
            &recursive,
            vec![Ty::Iter(Box::new(Ty::Parameter {
                owner: recursive.clone(),
                position: 0,
            }))],
        )],
    );
    let receiver = Ty::SelfType(recursive.clone());
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
fn standard_ids_require_carried_contracts_and_obey_cancellation() {
    for kind in Protocol::ALL {
        let owner = language::identity(kind);
        let root = applied(&owner, vec![]);
        assert_eq!(
            trait_closure(
                &root,
                &Ty::SelfType(owner.clone()),
                &CancellationToken::default(),
                &|_| None
            ),
            Err(TypeTransformError::InvalidContract),
            "{}",
            kind.name()
        );
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert_eq!(
            trait_closure(&root, &Ty::SelfType(owner), &cancel, &|_| None),
            Err(TypeTransformError::Cancelled)
        );
    }
}

#[test]
fn executable_trait_lookup_requires_exact_owner_and_declaration_identity() {
    let owner = id("Read");
    let record = contract(&owner, 0, vec![]);
    let public = [PublicItem::Trait(record.clone())];
    assert_eq!(
        trait_contract(&owner.module, &public, &[], &owner),
        Some(&record)
    );
    let private = [TraitContract {
        declaration: owner.clone(),
        abi: record.clone(),
    }];
    assert_eq!(
        trait_contract(&owner.module, &[], &private, &owner),
        Some(&record)
    );
    let mut foreign = owner.clone();
    foreign.module = ModuleIdentity::single_file("other.kgr");
    assert!(trait_contract(&owner.module, &public, &private, &foreign).is_none());
    let mut repeated = owner.clone();
    repeated.path[0].occurrence = 1;
    assert!(trait_contract(&owner.module, &public, &private, &repeated).is_none());
    let standard = language::identity(Protocol::PartialEq);
    assert!(trait_contract(&standard.module, &[], &[], &standard).is_none());
}
