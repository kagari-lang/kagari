use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::ty::inheritance::*;
use kagari_types::{
    declaration::TraitDef,
    ty::{NominalTy, Ty, substitution::TypeTransformError},
};
use std::collections::BTreeMap;
use {
    crate::types::{PublicItem, TraitContract, trait_contract},
    kagari_types::{language, language::Protocol, scalar::BuiltinType, ty::GenericParam},
};

fn id(context: &DefinitionContext, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file("inheritance.kgr"))
        .unwrap();
    context
        .intern_child(root, DefinitionKind::Trait, name, 0)
        .unwrap()
}

fn applied(owner: &DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: *owner,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn contract(
    context: &DefinitionContext,
    owner: &DefinitionId,
    arity: usize,
    parents: Vec<NominalTy>,
) -> TraitDef {
    TraitDef {
        conversion_adapter: None,
        name: context
            .snapshot()
            .resolve(*owner)
            .unwrap()
            .last()
            .unwrap()
            .name
            .to_owned(),
        generic_params: (0..arity)
            .map(|position| GenericParam {
                owner: *owner,
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
    let context = DefinitionContext::new().unwrap();
    let derived = id(&context, "Derived");
    let left = id(&context, "Left");
    let right = id(&context, "Right");
    let base = id(&context, "Base");
    let item = context
        .intern_child(derived, DefinitionKind::AssociatedType, "Item", 0)
        .unwrap();
    let parameter = Ty::Parameter {
        owner: derived,
        position: 0,
    };
    let mut root = applied(&derived, vec![Ty::Builtin(BuiltinType::I32)]);
    root.associated_types
        .insert(item, Ty::Builtin(BuiltinType::Bool));
    let projection = Ty::Projection {
        receiver: Box::new(Ty::SelfType(derived)),
        interface: Box::new(applied(&derived, vec![parameter.clone()])),
        member: item,
        arguments: vec![],
    };
    let receiver = Ty::Builtin(BuiltinType::String);
    let input = Ty::Tuple(vec![parameter, projection, Ty::SelfType(derived)]);
    let records: BTreeMap<_, _> = [
        (
            derived,
            contract(
                &context,
                &derived,
                1,
                vec![
                    applied(&left, vec![input.clone()]),
                    applied(&right, vec![input]),
                ],
            ),
        ),
        (
            left,
            contract(
                &context,
                &left,
                1,
                vec![applied(
                    &base,
                    vec![Ty::Parameter {
                        owner: left,
                        position: 0,
                    }],
                )],
            ),
        ),
        (
            right,
            contract(
                &context,
                &right,
                1,
                vec![applied(
                    &base,
                    vec![Ty::Parameter {
                        owner: right,
                        position: 0,
                    }],
                )],
            ),
        ),
        (base, contract(&context, &base, 1, vec![])),
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
    let context = DefinitionContext::new().unwrap();
    let recursive = id(&context, "Recursive");
    let root = applied(&recursive, vec![Ty::Builtin(BuiltinType::I32)]);
    let record = contract(
        &context,
        &recursive,
        1,
        vec![applied(
            &recursive,
            vec![Ty::Iter(Box::new(Ty::Parameter {
                owner: recursive,
                position: 0,
            }))],
        )],
    );
    let receiver = Ty::SelfType(recursive);
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
    let context = DefinitionContext::new().unwrap();
    for kind in Protocol::ALL {
        let owner = context.intern(&language::identity(kind)).unwrap();
        let root = applied(&owner, vec![]);
        assert_eq!(
            trait_closure(
                &root,
                &Ty::SelfType(owner),
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
    let context = DefinitionContext::new().unwrap();
    let owner = id(&context, "Read");
    let record = contract(&context, &owner, 0, vec![]);
    let public = [PublicItem::Trait(record.clone())];
    assert_eq!(
        trait_contract(
            &ModuleIdentity::single_file("inheritance.kgr"),
            &public,
            &[],
            &owner,
            &context.snapshot()
        ),
        Some(&record)
    );
    let private = [TraitContract {
        declaration: owner,
        abi: record.clone(),
    }];
    assert_eq!(
        trait_contract(
            &ModuleIdentity::single_file("inheritance.kgr"),
            &[],
            &private,
            &owner,
            &context.snapshot()
        ),
        Some(&record)
    );
    let foreign = context
        .intern_child(
            context
                .intern_root(&ModuleIdentity::single_file("other.kgr"))
                .unwrap(),
            DefinitionKind::Trait,
            "Read",
            0,
        )
        .unwrap();
    assert!(
        trait_contract(
            &ModuleIdentity::single_file("inheritance.kgr"),
            &public,
            &private,
            &foreign,
            &context.snapshot()
        )
        .is_none()
    );
    let repeated = context
        .intern_child(
            context.snapshot().parent(owner).unwrap().unwrap(),
            DefinitionKind::Trait,
            "Read",
            1,
        )
        .unwrap();
    assert!(
        trait_contract(
            &ModuleIdentity::single_file("inheritance.kgr"),
            &public,
            &private,
            &repeated,
            &context.snapshot()
        )
        .is_none()
    );
    let standard = context
        .intern(&language::identity(Protocol::PartialEq))
        .unwrap();
    assert!(
        trait_contract(
            context.snapshot().resolve(standard).unwrap().module(),
            &[],
            &[],
            &standard,
            &context.snapshot()
        )
        .is_none()
    );
}
