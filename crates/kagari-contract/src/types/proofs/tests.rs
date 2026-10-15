use super::*;
use crate::{language::primitive as intrinsic, layout::EnumVariantLayout, types::InterfaceTable};
use kagari_common::identity::{
    DefinitionKind, ModuleIdentity, map::DefinitionContext, table::DefinitionId,
};
use kagari_types::{
    collection::CollectionAccess,
    declaration::{
        AssociatedTypeFamily, TypeDef, TypeDefKind, module::ImplDecl,
        native::NativeTypeConstructor, ownership::ReceiverOwners,
    },
    language::{self, Protocol, binding},
    scalar::BuiltinType,
    ty::GenericParam,
};
use std::iter;

// These names support intrinsic queries but carry no trait or implementation proof.
fn proof_context() -> DefinitionContext {
    let context = DefinitionContext::new().unwrap();
    for protocol in Protocol::ALL {
        context.intern(&language::identity(protocol)).unwrap();
    }
    for (protocol, names) in [
        (Protocol::Iterator, &["Item"][..]),
        (Protocol::Iterable, &["Item", "Iter"][..]),
    ] {
        let owner = context.lookup(&language::identity(protocol)).unwrap();
        for name in names {
            context
                .intern_child(owner, DefinitionKind::AssociatedType, name, 0)
                .unwrap();
        }
    }
    context
}

fn id(context: &DefinitionContext, kind: DefinitionKind, name: &str) -> DefinitionId {
    let root = context
        .intern_root(&ModuleIdentity::single_file("proof.kgr"))
        .unwrap();
    context.intern_child(root, kind, name, 0).unwrap()
}

fn nominal(id: DefinitionId, arguments: Vec<Ty>) -> NominalTy {
    NominalTy {
        declaration: id,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn wrapper_layout(context: &DefinitionContext, payload: Ty) -> EnumLayout {
    let declaration = context.intern(&binding::option_declaration()).unwrap();
    EnumLayout {
        declaration,
        arguments: vec![payload.clone()],
        variants: vec![
            EnumVariantLayout {
                reports_failure: false,
                declaration: context
                    .intern_child(declaration, DefinitionKind::Variant, "Some", 0)
                    .unwrap(),
                payload: vec![payload],
            },
            EnumVariantLayout {
                reports_failure: false,
                declaration: context
                    .intern_child(declaration, DefinitionKind::Variant, "None", 0)
                    .unwrap(),
                payload: vec![],
            },
        ],
    }
}

fn scalar() -> Ty {
    Ty::Builtin(BuiltinType::I32)
}

fn table(
    context: &DefinitionContext,
    name: &str,
    interface: NominalTy,
    receiver: Ty,
) -> InterfaceTable {
    InterfaceTable {
        declaration: id(context, DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: Ty::Trait(interface),
        for_type: receiver,
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        bridge: None,
    }
}

fn bound(ty: Ty, required: NominalTy) -> GenericBound {
    GenericBound {
        ty,
        constraints: vec![Constraint::Trait(required)],
    }
}

#[test]
fn scalar_aggregation_is_not_a_source_free_intrinsic_proof() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let interfaces = ["Sum", "Product"]
        .map(|name| nominal(id(&context, DefinitionKind::Trait, name), vec![scalar()]));
    let catalog = ProofCatalog::new(vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    for interface in interfaces {
        assert!(
            intrinsic::requirements(&interface, &scalar(), &cancel, None, &context.snapshot())
                .unwrap()
                .is_none()
        );
        assert!(!catalog.holds(&interface, &scalar(), &[], &cancel).unwrap());
    }
}

#[test]
fn linked_proofs_discharge_generic_bounds_and_reject_ambiguity_and_cycles() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let marker = nominal(id(&context, DefinitionKind::Trait, "Marker"), vec![]);
    let mut generic = table(&context, "generic", marker.clone(), scalar());
    let parameter = GenericParam {
        owner: generic.declaration,
        position: 0,
    };
    generic.for_type = Ty::Array(Box::new(parameter.as_type()));
    generic.generic_params.push(parameter.clone());
    generic.bounds.push(bound(
        parameter.as_type(),
        intrinsic::applied(Protocol::Eq, vec![], &context.snapshot()).unwrap(),
    ));
    let query = |item| Ty::Array(Box::new(item));
    let catalog = ProofCatalog::new(
        vec![(&generic).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        catalog
            .holds(&marker, &query(scalar()), &[], &cancel)
            .unwrap()
    );
    assert!(
        !catalog
            .holds(&marker, &query(Ty::Builtin(BuiltinType::F32)), &[], &cancel)
            .unwrap()
    );
    let another = table(&context, "concrete", marker.clone(), query(scalar()));
    let catalog = ProofCatalog::new(
        vec![(&generic).into(), (&another).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &query(scalar()), &[], &cancel)
            .unwrap(),
        2
    );
    assert!(
        !catalog
            .holds(&marker, &query(scalar()), &[], &cancel)
            .unwrap()
    );
    let mut recursive = table(&context, "recursive", marker.clone(), scalar());
    recursive.bounds.push(bound(scalar(), marker.clone()));
    let catalog = ProofCatalog::new(
        vec![(&recursive).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(!catalog.holds(&marker, &scalar(), &[], &cancel).unwrap());
    cancel.cancel();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn registered_collection_equality_requires_explicit_prerequisites() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let module = ModuleIdentity::single_file("proof.kgr");
    let declaration = TypeDef {
        name: "HashSet".into(),
        kind: TypeDefKind::Native(NativeTypeConstructor::Set),
        generic_params: vec![GenericParam {
            owner: id(&context, DefinitionKind::AssociatedType, "HashSet"),
            position: 0,
        }],
        bounds: vec![],
        fields: vec![],
        variants: vec![],
    };
    let owners = ReceiverOwners::from_types([(&module, &declaration)]).unwrap();
    let receiver = Ty::Set(Box::new(scalar()), CollectionAccess::Mutable);
    let tables = [Protocol::PartialEq, Protocol::Eq, Protocol::Hash].map(|protocol| {
        table(
            &context,
            protocol.name(),
            intrinsic::applied(protocol, vec![], &context.snapshot()).unwrap(),
            receiver.clone(),
        )
    });
    for (indices, valid) in [
        (&[0][..], true),
        (&[1][..], false),
        (&[2][..], false),
        (&[0, 2][..], false),
        (&[0, 1, 2][..], true),
    ] {
        let catalog = ProofCatalog::new(
            indices
                .iter()
                .map(|index| (&tables[*index]).into())
                .collect(),
            [],
            [],
            [],
            &context.snapshot(),
            &cancel,
        )
        .unwrap();
        assert_eq!(catalog.overrides_valid(&owners, &cancel).unwrap(), valid);
        assert!(
            !catalog
                .overrides_valid(&Default::default(), &cancel)
                .unwrap()
        );
    }
}

#[test]
fn nominal_equality_overrides_disable_defaults_and_require_owned_explicit_prerequisites() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let receiver = Ty::Struct(nominal(id(&context, DefinitionKind::Struct, "Key"), vec![]));
    let partial = table(
        &context,
        "partial",
        intrinsic::applied(Protocol::PartialEq, vec![], &context.snapshot()).unwrap(),
        receiver.clone(),
    );
    let eq = table(
        &context,
        "eq",
        intrinsic::applied(Protocol::Eq, vec![], &context.snapshot()).unwrap(),
        receiver.clone(),
    );
    let hash = table(
        &context,
        "hash",
        intrinsic::applied(Protocol::Hash, vec![], &context.snapshot()).unwrap(),
        receiver.clone(),
    );
    let catalog = ProofCatalog::new(
        vec![(&partial).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        catalog
            .overrides_valid(&Default::default(), &cancel)
            .unwrap()
    );
    assert!(
        !catalog
            .holds(
                &intrinsic::applied(Protocol::Eq, vec![], &context.snapshot()).unwrap(),
                &receiver,
                &[],
                &cancel
            )
            .unwrap()
    );
    let catalog = ProofCatalog::new(
        vec![(&partial).into(), (&hash).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        !catalog
            .overrides_valid(&Default::default(), &cancel)
            .unwrap()
    );
    let catalog = ProofCatalog::new(
        vec![(&partial).into(), (&eq).into(), (&hash).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        catalog
            .overrides_valid(&Default::default(), &cancel)
            .unwrap()
    );
    let mut foreign = partial.clone();
    foreign.declaration = context
        .intern_child(
            context
                .intern_root(&ModuleIdentity::single_file("foreign.kgr"))
                .unwrap(),
            DefinitionKind::Impl,
            "partial",
            0,
        )
        .unwrap();
    assert!(
        !ProofCatalog::new(
            vec![(&foreign).into()],
            [],
            [],
            [],
            &context.snapshot(),
            &cancel
        )
        .unwrap()
        .overrides_valid(&Default::default(), &cancel)
        .unwrap()
    );
    assert!(
        ProofCatalog::new(
            vec![(&partial).into(), (&partial).into()],
            [],
            [],
            [],
            &context.snapshot(),
            &cancel
        )
        .is_err()
    );
}

#[test]
fn family_projection_normalization_applies_both_binders_and_rejects_cycles() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let interface = nominal(id(&context, DefinitionKind::Trait, "Project"), vec![]);
    let member = context
        .intern_child(
            interface.declaration,
            DefinitionKind::AssociatedType,
            "Item",
            0,
        )
        .unwrap();
    let mut implementation = table(&context, "family", interface.clone(), scalar());
    let parameter = GenericParam {
        owner: implementation.declaration,
        position: 0,
    };
    implementation.generic_params.push(parameter.clone());
    implementation.for_type = Ty::Array(Box::new(parameter.as_type()));
    let input = GenericParam {
        owner: member,
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamily {
            declaration: member,
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: Ty::Tuple(vec![parameter.as_type(), input.as_type()]),
        });
    let projection = Ty::Projection {
        receiver: Box::new(Ty::Array(Box::new(scalar()))),
        interface: Box::new(interface.clone()),
        member,
        arguments: vec![Ty::Builtin(BuiltinType::Bool)],
    };
    let catalog = ProofCatalog::new(
        vec![(&implementation).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        catalog.normalize(&projection, &cancel).unwrap(),
        Ty::Tuple(vec![scalar(), Ty::Builtin(BuiltinType::Bool)])
    );
    implementation.associated_type_families[0].value = projection.clone();
    let catalog = ProofCatalog::new(
        vec![(&implementation).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(catalog.normalize(&projection, &cancel).is_err());
}

#[test]
fn member_defaults_are_limited_to_core_enums_and_proven_payloads() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let user = nominal(id(&context, DefinitionKind::Enum, "Choice"), vec![]);
    let user_layout = EnumLayout {
        declaration: user.declaration,
        arguments: vec![],
        variants: vec![],
    };
    let object = Ty::Struct(nominal(
        id(&context, DefinitionKind::Struct, "Object"),
        vec![],
    ));
    for (payload, partial, key) in [
        (scalar(), true, true),
        (Ty::Builtin(BuiltinType::F64), true, false),
        (Ty::Enum(user.clone()), false, false),
        (object, false, false),
        (Ty::Array(Box::new(scalar())), false, false),
        (
            Ty::Set(Box::new(scalar()), CollectionAccess::Mutable),
            false,
            false,
        ),
    ] {
        let layout = wrapper_layout(&context, payload.clone());
        let wrapper = Ty::Enum(nominal(layout.declaration, vec![payload]));
        let catalog = ProofCatalog::new(
            vec![],
            [&layout, &user_layout],
            [],
            [],
            &context.snapshot(),
            &cancel,
        )
        .unwrap();
        for (protocol, expected) in [
            (Protocol::PartialEq, partial),
            (Protocol::Eq, key),
            (Protocol::Hash, key),
        ] {
            assert_eq!(
                catalog
                    .holds(
                        &intrinsic::applied(protocol, vec![], &context.snapshot()).unwrap(),
                        &wrapper,
                        &[],
                        &cancel
                    )
                    .unwrap(),
                expected,
                "{protocol:?}: {wrapper:?}"
            );
        }
    }
}

#[test]
fn recursive_growth_is_bounded_and_host_candidates_participate_in_uniqueness() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let marker = nominal(id(&context, DefinitionKind::Trait, "Marker"), vec![]);
    let mut growing = table(&context, "growing", marker.clone(), scalar());
    let parameter = GenericParam {
        owner: growing.declaration,
        position: 0,
    };
    growing.for_type = parameter.as_type();
    growing.generic_params.push(parameter.clone());
    growing.bounds.push(bound(
        Ty::Array(Box::new(parameter.as_type())),
        marker.clone(),
    ));
    let catalog = ProofCatalog::new(
        vec![(&growing).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::LimitExceeded)
    );
    let host = id(&context, DefinitionKind::Struct, "proof_host");
    let member = context
        .intern_child(
            marker.declaration,
            DefinitionKind::AssociatedType,
            "Item",
            0,
        )
        .unwrap();
    let receiver = Ty::Host(host);
    let mut applied = marker.clone();
    applied.associated_types.insert(member, scalar());
    let native_id = id(&context, DefinitionKind::Impl, "host_native");
    let native = ImplDecl {
        generic_params: vec![],
        bounds: vec![],
        trait_type: Some(applied.clone()),
        for_type: receiver.clone(),
        methods: vec![],
    };
    let catalog = ProofCatalog::new(
        vec![Implementation::Native {
            declaration: &native_id,
            implementation: &native,
        }],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &receiver, &[], &cancel)
            .unwrap(),
        1
    );
    let projection = Ty::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(marker.clone()),
        member,
        arguments: vec![],
    };
    assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), scalar());
    let script = table(&context, "host_script", applied, receiver.clone());
    let catalog = ProofCatalog::new(
        vec![
            (&script).into(),
            Implementation::Native {
                declaration: &native_id,
                implementation: &native,
            },
        ],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &receiver, &[], &cancel)
            .unwrap(),
        2
    );
    assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), projection);
}

#[test]
fn carried_native_implementations_preserve_key_bounds_and_wrapper_lifting() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let mut set = table(
        &context,
        "collect_set",
        nominal(id(&context, DefinitionKind::Trait, "FromIterator"), vec![]),
        scalar(),
    );
    let item = GenericParam {
        owner: set.declaration,
        position: 0,
    };
    set.generic_params.push(item.clone());
    set.for_type = Ty::Set(Box::new(item.as_type()), CollectionAccess::Mutable);
    set.trait_type = Ty::Trait(nominal(
        id(&context, DefinitionKind::Trait, "FromIterator"),
        vec![item.as_type()],
    ));
    for kind in [Protocol::Eq, Protocol::Hash] {
        set.bounds.push(bound(
            item.as_type(),
            intrinsic::applied(kind, vec![], &context.snapshot()).unwrap(),
        ));
    }
    let mut lifted = table(
        &context,
        "collect_option",
        nominal(id(&context, DefinitionKind::Trait, "FromIterator"), vec![]),
        scalar(),
    );
    let element = GenericParam {
        owner: lifted.declaration,
        position: 0,
    };
    let output = GenericParam {
        owner: lifted.declaration,
        position: 1,
    };
    lifted.generic_params = vec![element.clone(), output.clone()];
    let option = |ty| {
        Ty::Enum(NominalTy {
            declaration: context.intern(&binding::option_declaration()).unwrap(),
            arguments: vec![ty],
            associated_types: Default::default(),
        })
    };
    lifted.for_type = option(output.as_type());
    lifted.trait_type = Ty::Trait(nominal(
        id(&context, DefinitionKind::Trait, "FromIterator"),
        vec![option(element.as_type())],
    ));
    lifted.bounds.push(bound(
        output.as_type(),
        nominal(
            id(&context, DefinitionKind::Trait, "FromIterator"),
            vec![element.as_type()],
        ),
    ));
    let storage = |ty| Ty::Set(Box::new(ty), CollectionAccess::Mutable);
    let requested = |ty| {
        nominal(
            id(&context, DefinitionKind::Trait, "FromIterator"),
            vec![ty],
        )
    };
    let empty = ProofCatalog::new(vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    assert!(
        !empty
            .holds(&requested(scalar()), &storage(scalar()), &[], &cancel)
            .unwrap()
    );
    let catalog = ProofCatalog::new(
        vec![(&set).into(), (&lifted).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(
        catalog
            .holds(&requested(scalar()), &storage(scalar()), &[], &cancel)
            .unwrap()
    );
    assert!(
        catalog
            .holds(
                &requested(option(scalar())),
                &option(storage(scalar())),
                &[],
                &cancel
            )
            .unwrap()
    );
    let float = Ty::Builtin(BuiltinType::F64);
    assert!(
        !catalog
            .holds(
                &requested(float.clone()),
                &storage(float.clone()),
                &[],
                &cancel
            )
            .unwrap()
    );
    assert!(
        !catalog
            .holds(
                &requested(option(float.clone())),
                &option(storage(float)),
                &[],
                &cancel
            )
            .unwrap()
    );
    assert!(
        !catalog
            .holds(
                &requested(Ty::Builtin(BuiltinType::Bool)),
                &storage(scalar()),
                &[],
                &cancel
            )
            .unwrap()
    );
    cancel.cancel();
    assert_eq!(
        catalog.holds(&requested(scalar()), &storage(scalar()), &[], &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn carried_iterator_outputs_supply_identity_iterable_and_reject_forged_items() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let iterator = Ty::Iter(Box::new(scalar()));
    let mut implemented =
        intrinsic::applied(Protocol::Iterator, vec![], &context.snapshot()).unwrap();
    implemented.associated_types.insert(
        context
            .intern_child(
                implemented.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap(),
        scalar(),
    );
    let implementation = table(&context, "iterator", implemented, iterator.clone());
    let mut required = intrinsic::applied(Protocol::Iterable, vec![], &context.snapshot()).unwrap();
    required.associated_types.insert(
        context
            .intern_child(
                required.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap(),
        scalar(),
    );
    required.associated_types.insert(
        context
            .intern_child(
                required.declaration,
                DefinitionKind::AssociatedType,
                "Iter",
                0,
            )
            .unwrap(),
        iterator.clone(),
    );
    let catalog = ProofCatalog::new(
        vec![(&implementation).into()],
        [],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(catalog.holds(&required, &iterator, &[], &cancel).unwrap());
    for (member, value) in &required.associated_types {
        let projection = Ty::Projection {
            receiver: Box::new(iterator.clone()),
            interface: Box::new(required.clone()),
            member: *member,
            arguments: vec![],
        };
        assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), *value);
    }
    required.associated_types.insert(
        context
            .intern_child(
                required.declaration,
                DefinitionKind::AssociatedType,
                "Item",
                0,
            )
            .unwrap(),
        Ty::Builtin(BuiltinType::Bool),
    );
    assert!(!catalog.holds(&required, &iterator, &[], &cancel).unwrap());
}

#[test]
fn equality_composition_uses_carried_payloads_and_explicit_storage_implementations() {
    let context = proof_context();
    let cancel = CancellationToken::default();
    let key = Ty::Struct(nominal(id(&context, DefinitionKind::Struct, "Key"), vec![]));
    let partial = table(
        &context,
        "key_partial",
        intrinsic::applied(Protocol::PartialEq, vec![], &context.snapshot()).unwrap(),
        key.clone(),
    );
    let instance = nominal(id(&context, DefinitionKind::Enum, "Chain"), vec![]);
    let chain = Ty::Enum(instance.clone());
    let layout = EnumLayout {
        declaration: instance.declaration,
        arguments: vec![],
        variants: vec![EnumVariantLayout {
            reports_failure: false,
            declaration: id(&context, DefinitionKind::Variant, "Next"),
            payload: vec![key.clone(), chain.clone()],
        }],
    };
    let catalog = ProofCatalog::new(
        vec![(&partial).into()],
        [&layout],
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    assert!(catalog.uses_custom_equality(&chain, &cancel).unwrap());
    assert!(
        catalog
            .uses_custom_equality(&Ty::Tuple(vec![chain.clone()]), &cancel)
            .unwrap()
    );
    let storages = [
        Ty::Array(Box::new(key.clone())),
        Ty::Map {
            key: Box::new(key.clone()),
            value: Box::new(chain.clone()),
            access: CollectionAccess::ReadOnly,
        },
        Ty::Set(Box::new(key), CollectionAccess::Mutable),
    ];
    let wrappers = storages
        .iter()
        .cloned()
        .map(|ty| wrapper_layout(&context, ty))
        .collect::<Vec<_>>();
    let catalog = ProofCatalog::new(
        vec![(&partial).into()],
        iter::once(&layout).chain(wrappers.iter()),
        [],
        [],
        &context.snapshot(),
        &cancel,
    )
    .unwrap();
    for storage in storages {
        assert!(
            !catalog
                .uses_custom_equality(
                    &Ty::Enum(NominalTy {
                        declaration: context.intern(&binding::option_declaration()).unwrap(),
                        arguments: vec![storage],
                        associated_types: Default::default()
                    }),
                    &cancel
                )
                .unwrap()
        );
    }
    let empty = ProofCatalog::new(vec![], [], [], [], &context.snapshot(), &cancel).unwrap();
    assert_eq!(
        empty.uses_custom_equality(&chain, &cancel),
        Err(TypeTransformError::InvalidContract)
    );
    let mut deep = scalar();
    for _ in 0..=MAX_DEPTH {
        deep = Ty::Tuple(vec![deep]);
    }
    assert_eq!(
        empty.uses_custom_equality(&deep, &cancel),
        Err(TypeTransformError::LimitExceeded)
    );
    cancel.cancel();
    assert_eq!(
        catalog.uses_custom_equality(&chain, &cancel),
        Err(TypeTransformError::Cancelled)
    );
}
