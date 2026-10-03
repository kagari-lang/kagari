use super::*;
use crate::{
    language::{Protocol, primitive as intrinsic},
    layout::EnumVariantLayout,
    scalar::BuiltinType,
    standard::surface::StandardEnum,
    types::{AssociatedTypeFamilyAbi, GenericParameterAbi, InterfaceTableAbi},
};
use kagari_common::{
    collection::CollectionAccess,
    identity::{DefinitionKind, DefinitionPathSegment, ModuleIdentity, associated_type_id},
};

fn id(kind: DefinitionKind, name: &str) -> DefinitionId {
    DefinitionId {
        module: ModuleIdentity::single_file("proof.kgr"),
        path: vec![DefinitionPathSegment {
            kind,
            name: name.into(),
            occurrence: 0,
        }],
    }
}

fn nominal(id: DefinitionId, arguments: Vec<AbiType>) -> NominalAbiType {
    NominalAbiType {
        declaration: id,
        arguments,
        associated_types: BTreeMap::new(),
    }
}

fn scalar() -> AbiType {
    AbiType::Builtin(BuiltinType::I32)
}

fn table(name: &str, interface: NominalAbiType, receiver: AbiType) -> InterfaceTableAbi {
    InterfaceTableAbi {
        declaration: id(DefinitionKind::Impl, name),
        name: name.into(),
        generic_params: vec![],
        bounds: vec![],
        trait_type: AbiType::Trait(interface),
        for_type: receiver,
        methods: vec![],
        associated_consts: vec![],
        associated_type_families: vec![],
        host_bridge: false,
    }
}

fn bound(ty: AbiType, required: NominalAbiType) -> GenericBoundAbi {
    GenericBoundAbi {
        ty,
        constraints: vec![ConstraintAbi::Trait(required)],
    }
}

#[test]
fn scalar_aggregation_is_not_a_source_free_intrinsic_proof() {
    let cancel = CancellationToken::default();
    let catalog = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    for name in ["Sum", "Product"] {
        let interface = nominal(id(DefinitionKind::Trait, name), vec![scalar()]);
        assert!(
            intrinsic::requirements(&interface, &scalar(), &cancel)
                .unwrap()
                .is_none()
        );
        assert!(!catalog.holds(&interface, &scalar(), &[], &cancel).unwrap());
    }
}

#[test]
fn linked_proofs_discharge_generic_bounds_and_reject_ambiguity_and_cycles() {
    let cancel = CancellationToken::default();
    let marker = nominal(id(DefinitionKind::Trait, "Marker"), vec![]);
    let mut generic = table("generic", marker.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: generic.declaration.clone(),
        position: 0,
    };
    generic.for_type = AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable);
    generic.generic_params.push(parameter.clone());
    generic.bounds.push(bound(
        parameter.as_type(),
        intrinsic::applied(Protocol::Eq, vec![]),
    ));
    let query = |item| AbiType::Array(Box::new(item), CollectionAccess::Mutable);
    let catalog = ProofCatalog::new(vec![(&generic).into()], vec![], [], [], [], &cancel).unwrap();
    assert!(
        catalog
            .holds(&marker, &query(scalar()), &[], &cancel)
            .unwrap()
    );
    assert!(
        !catalog
            .holds(
                &marker,
                &query(AbiType::Builtin(BuiltinType::F32)),
                &[],
                &cancel
            )
            .unwrap()
    );
    let another = table("concrete", marker.clone(), query(scalar()));
    let catalog = ProofCatalog::new(
        vec![(&generic).into(), (&another).into()],
        vec![],
        [],
        [],
        [],
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
    let mut recursive = table("recursive", marker.clone(), scalar());
    recursive.bounds.push(bound(scalar(), marker.clone()));
    let catalog =
        ProofCatalog::new(vec![(&recursive).into()], vec![], [], [], [], &cancel).unwrap();
    assert!(!catalog.holds(&marker, &scalar(), &[], &cancel).unwrap());
    cancel.cancel();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::Cancelled)
    );
}

#[test]
fn nominal_equality_overrides_disable_defaults_and_require_owned_explicit_prerequisites() {
    let cancel = CancellationToken::default();
    let receiver = AbiType::Struct(nominal(id(DefinitionKind::Struct, "Key"), vec![]));
    let partial = table(
        "partial",
        intrinsic::applied(Protocol::PartialEq, vec![]),
        receiver.clone(),
    );
    let eq = table(
        "eq",
        intrinsic::applied(Protocol::Eq, vec![]),
        receiver.clone(),
    );
    let hash = table(
        "hash",
        intrinsic::applied(Protocol::Hash, vec![]),
        receiver.clone(),
    );
    let catalog = ProofCatalog::new(vec![(&partial).into()], vec![], [], [], [], &cancel).unwrap();
    assert!(catalog.overrides_valid(&cancel).unwrap());
    assert!(
        !catalog
            .holds(
                &intrinsic::applied(Protocol::Eq, vec![]),
                &receiver,
                &[],
                &cancel
            )
            .unwrap()
    );
    let catalog = ProofCatalog::new(
        vec![(&partial).into(), (&hash).into()],
        vec![],
        [],
        [],
        [],
        &cancel,
    )
    .unwrap();
    assert!(!catalog.overrides_valid(&cancel).unwrap());
    let catalog = ProofCatalog::new(
        vec![(&partial).into(), (&eq).into(), (&hash).into()],
        vec![],
        [],
        [],
        [],
        &cancel,
    )
    .unwrap();
    assert!(catalog.overrides_valid(&cancel).unwrap());
    let mut foreign = partial.clone();
    foreign.declaration.module = ModuleIdentity::single_file("foreign.kgr");
    assert!(
        !ProofCatalog::new(vec![(&foreign).into()], vec![], [], [], [], &cancel)
            .unwrap()
            .overrides_valid(&cancel)
            .unwrap()
    );
    assert!(
        ProofCatalog::new(
            vec![(&partial).into(), (&partial).into()],
            vec![],
            [],
            [],
            [],
            &cancel
        )
        .is_err()
    );
}

#[test]
fn family_projection_normalization_applies_both_binders_and_rejects_cycles() {
    let cancel = CancellationToken::default();
    let interface = nominal(id(DefinitionKind::Trait, "Project"), vec![]);
    let member = associated_type_id(&interface.declaration, "Item");
    let mut implementation = table("family", interface.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: implementation.declaration.clone(),
        position: 0,
    };
    implementation.generic_params.push(parameter.clone());
    implementation.for_type =
        AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable);
    let input = GenericParameterAbi {
        owner: member.clone(),
        position: 0,
    };
    implementation
        .associated_type_families
        .push(AssociatedTypeFamilyAbi {
            declaration: member.clone(),
            generic_params: vec![input.clone()],
            bounds: vec![],
            value: AbiType::Tuple(vec![parameter.as_type(), input.as_type()]),
        });
    let projection = AbiType::Projection {
        receiver: Box::new(AbiType::Array(
            Box::new(scalar()),
            CollectionAccess::Mutable,
        )),
        interface: Box::new(interface.clone()),
        member: member.clone(),
        arguments: vec![AbiType::Builtin(BuiltinType::Bool)],
    };
    let catalog =
        ProofCatalog::new(vec![(&implementation).into()], vec![], [], [], [], &cancel).unwrap();
    assert_eq!(
        catalog.normalize(&projection, &cancel).unwrap(),
        AbiType::Tuple(vec![scalar(), AbiType::Builtin(BuiltinType::Bool)])
    );
    implementation.associated_type_families[0].value = projection.clone();
    let catalog =
        ProofCatalog::new(vec![(&implementation).into()], vec![], [], [], [], &cancel).unwrap();
    assert!(catalog.normalize(&projection, &cancel).is_err());
}

#[test]
fn structural_enum_defaults_are_coinductive_but_reject_non_hashable_payloads() {
    let cancel = CancellationToken::default();
    let instance = nominal(id(DefinitionKind::Enum, "Chain"), vec![]);
    let ty = AbiType::Enum(instance.clone());
    let mut layout = EnumLayout {
        declaration: instance.declaration,
        arguments: vec![],
        variants: vec![EnumVariantLayout {
            declaration: id(DefinitionKind::Variant, "Next"),
            payload: vec![ty.clone()],
        }],
    };
    let catalog = ProofCatalog::new(vec![], vec![], [&layout], [], [], &cancel).unwrap();
    assert!(
        catalog
            .holds(
                &intrinsic::applied(Protocol::Hash, vec![]),
                &ty,
                &[],
                &cancel
            )
            .unwrap()
    );
    layout.variants[0]
        .payload
        .push(AbiType::Builtin(BuiltinType::F64));
    let catalog = ProofCatalog::new(vec![], vec![], [&layout], [], [], &cancel).unwrap();
    assert!(
        !catalog
            .holds(
                &intrinsic::applied(Protocol::Hash, vec![]),
                &ty,
                &[],
                &cancel
            )
            .unwrap()
    );
    let set = AbiType::Set(Box::new(ty), CollectionAccess::Mutable);
    let collect = nominal(
        id(DefinitionKind::Trait, "FromIterator"),
        vec![AbiType::Enum(nominal(layout.declaration.clone(), vec![]))],
    );
    assert!(!catalog.holds(&collect, &set, &[], &cancel).unwrap());
    // Storage itself remains an identity key regardless of element equality.
    let wrapped = AbiType::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![set],
    };
    assert!(
        catalog
            .holds(
                &intrinsic::applied(Protocol::Hash, vec![]),
                &wrapped,
                &[],
                &cancel
            )
            .unwrap()
    );
}

#[test]
fn recursive_growth_is_bounded_and_host_candidates_participate_in_uniqueness() {
    use kagari_common::host_interface::{
        type_declaration::HostAssociatedTypeBinding, value_type::HostValueType,
    };
    let cancel = CancellationToken::default();
    let marker = nominal(id(DefinitionKind::Trait, "Marker"), vec![]);
    let mut growing = table("growing", marker.clone(), scalar());
    let parameter = GenericParameterAbi {
        owner: growing.declaration.clone(),
        position: 0,
    };
    growing.for_type = parameter.as_type();
    growing.generic_params.push(parameter.clone());
    growing.bounds.push(bound(
        AbiType::Array(Box::new(parameter.as_type()), CollectionAccess::Mutable),
        marker.clone(),
    ));
    let catalog = ProofCatalog::new(vec![(&growing).into()], vec![], [], [], [], &cancel).unwrap();
    assert_eq!(
        catalog.holds(&marker, &scalar(), &[], &cancel),
        Err(TypeTransformError::LimitExceeded)
    );
    let mut host = HostTypeDeclaration::new("proof_host");
    let mut implementation =
        HostTraitImplementationDeclaration::new(marker.declaration.clone(), vec![], vec![]);
    let member = associated_type_id(&marker.declaration, "Item");
    implementation
        .associated_types
        .push(HostAssociatedTypeBinding {
            declaration: member.clone(),
            ty: HostValueType::I32,
        });
    host.trait_implementations.push(implementation);
    let receiver = AbiType::Host(host.id.clone());
    let catalog = ProofCatalog::new(vec![], vec![&host], [], [], [], &cancel).unwrap();
    assert_eq!(
        catalog
            .implementation_count(&marker, &receiver, &[], &cancel)
            .unwrap(),
        1
    );
    let projection = AbiType::Projection {
        receiver: Box::new(receiver.clone()),
        interface: Box::new(marker.clone()),
        member,
        arguments: vec![],
    };
    assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), scalar());
    let script = table(
        "host_script",
        host_application(&host.trait_implementations[0]),
        receiver.clone(),
    );
    let catalog =
        ProofCatalog::new(vec![(&script).into()], vec![&host], [], [], [], &cancel).unwrap();
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
    let cancel = CancellationToken::default();
    let mut set = table(
        "collect_set",
        nominal(id(DefinitionKind::Trait, "FromIterator"), vec![]),
        scalar(),
    );
    let item = GenericParameterAbi {
        owner: set.declaration.clone(),
        position: 0,
    };
    set.generic_params.push(item.clone());
    set.for_type = AbiType::Set(Box::new(item.as_type()), CollectionAccess::Mutable);
    set.trait_type = AbiType::Trait(nominal(
        id(DefinitionKind::Trait, "FromIterator"),
        vec![item.as_type()],
    ));
    for kind in [Protocol::Eq, Protocol::Hash] {
        set.bounds
            .push(bound(item.as_type(), intrinsic::applied(kind, vec![])));
    }
    let mut lifted = table(
        "collect_option",
        nominal(id(DefinitionKind::Trait, "FromIterator"), vec![]),
        scalar(),
    );
    let element = GenericParameterAbi {
        owner: lifted.declaration.clone(),
        position: 0,
    };
    let output = GenericParameterAbi {
        owner: lifted.declaration.clone(),
        position: 1,
    };
    lifted.generic_params = vec![element.clone(), output.clone()];
    let option = |ty| AbiType::StandardEnum {
        kind: StandardEnum::Option,
        args: vec![ty],
    };
    lifted.for_type = option(output.as_type());
    lifted.trait_type = AbiType::Trait(nominal(
        id(DefinitionKind::Trait, "FromIterator"),
        vec![option(element.as_type())],
    ));
    lifted.bounds.push(bound(
        output.as_type(),
        nominal(
            id(DefinitionKind::Trait, "FromIterator"),
            vec![element.as_type()],
        ),
    ));
    let storage = |ty| AbiType::Set(Box::new(ty), CollectionAccess::Mutable);
    let requested = |ty| nominal(id(DefinitionKind::Trait, "FromIterator"), vec![ty]);
    let empty = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    assert!(
        !empty
            .holds(&requested(scalar()), &storage(scalar()), &[], &cancel)
            .unwrap()
    );
    let catalog = ProofCatalog::new(
        vec![(&set).into(), (&lifted).into()],
        vec![],
        [],
        [],
        [],
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
    let float = AbiType::Builtin(BuiltinType::F64);
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
                &requested(AbiType::Builtin(BuiltinType::Bool)),
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
    let cancel = CancellationToken::default();
    let iterator = AbiType::Iter(Box::new(scalar()));
    let mut implemented = intrinsic::applied(Protocol::Iterator, vec![]);
    implemented.associated_types.insert(
        associated_type_id(&implemented.declaration, "Item"),
        scalar(),
    );
    let implementation = table("iterator", implemented, iterator.clone());
    let mut required = intrinsic::applied(Protocol::Iterable, vec![]);
    required
        .associated_types
        .insert(associated_type_id(&required.declaration, "Item"), scalar());
    required.associated_types.insert(
        associated_type_id(&required.declaration, "Iter"),
        iterator.clone(),
    );
    let catalog =
        ProofCatalog::new(vec![(&implementation).into()], vec![], [], [], [], &cancel).unwrap();
    assert!(catalog.holds(&required, &iterator, &[], &cancel).unwrap());
    for (member, value) in &required.associated_types {
        let projection = AbiType::Projection {
            receiver: Box::new(iterator.clone()),
            interface: Box::new(required.clone()),
            member: member.clone(),
            arguments: vec![],
        };
        assert_eq!(catalog.normalize(&projection, &cancel).unwrap(), *value);
    }
    required.associated_types.insert(
        associated_type_id(&required.declaration, "Item"),
        AbiType::Builtin(BuiltinType::Bool),
    );
    assert!(!catalog.holds(&required, &iterator, &[], &cancel).unwrap());
}

#[test]
fn equality_composition_uses_carried_payloads_and_stops_at_identity_boundaries() {
    let cancel = CancellationToken::default();
    let key = AbiType::Struct(nominal(id(DefinitionKind::Struct, "Key"), vec![]));
    let partial = table(
        "key_partial",
        intrinsic::applied(Protocol::PartialEq, vec![]),
        key.clone(),
    );
    let instance = nominal(id(DefinitionKind::Enum, "Chain"), vec![]);
    let chain = AbiType::Enum(instance.clone());
    let layout = EnumLayout {
        declaration: instance.declaration,
        arguments: vec![],
        variants: vec![EnumVariantLayout {
            declaration: id(DefinitionKind::Variant, "Next"),
            payload: vec![key.clone(), chain.clone()],
        }],
    };
    let catalog =
        ProofCatalog::new(vec![(&partial).into()], vec![], [&layout], [], [], &cancel).unwrap();
    assert!(catalog.uses_custom_equality(&chain, &cancel).unwrap());
    assert!(
        catalog
            .uses_custom_equality(&AbiType::Tuple(vec![chain.clone()]), &cancel)
            .unwrap()
    );
    for storage in [
        AbiType::Array(Box::new(key.clone()), CollectionAccess::Mutable),
        AbiType::Map {
            key: Box::new(key.clone()),
            value: Box::new(chain.clone()),
            access: CollectionAccess::ReadOnly,
        },
        AbiType::Set(Box::new(key), CollectionAccess::Mutable),
    ] {
        assert!(
            !catalog
                .uses_custom_equality(
                    &AbiType::StandardEnum {
                        kind: StandardEnum::Option,
                        args: vec![storage],
                    },
                    &cancel
                )
                .unwrap()
        );
    }
    let empty = ProofCatalog::new(vec![], vec![], [], [], [], &cancel).unwrap();
    assert_eq!(
        empty.uses_custom_equality(&chain, &cancel),
        Err(TypeTransformError::InvalidContract)
    );
    let mut deep = scalar();
    for _ in 0..=MAX_DEPTH {
        deep = AbiType::Tuple(vec![deep]);
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
