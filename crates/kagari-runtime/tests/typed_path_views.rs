use kagari_runtime::{
    AbiFingerprint, CapabilitySet, DynamicPathArgSlot, DynamicPathArgument, DynamicPathArguments,
    FieldMetadataId, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
    HostPathSegment, HostReflectionPolicy, HostSchemaEpoch, HostTypeOwnership,
    HostTypeRegistration, PathAccess, Runtime, RuntimeErrorKind, TypeId, TypeKind,
    TypeRegistration, value::Value,
};

fn register_i32(runtime: &Runtime) -> TypeId {
    runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(10),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap()
}

fn register_host_root_type(runtime: &mut Runtime, name: &str, access: PathAccess) -> TypeId {
    let mut registration = HostTypeRegistration::new(name, name);
    registration.ownership = HostTypeOwnership::HostRoot;
    registration.path_access = access;
    registration.reflection = HostReflectionPolicy::Metadata;
    registration.abi_fingerprint = AbiFingerprint(20);
    runtime.register_host_type(registration).unwrap()
}

#[test]
fn registers_typed_host_roots_and_simple_path_views() {
    let mut runtime = Runtime::default();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();

    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(21),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(22),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();

    let view = runtime
        .make_host_path_view(root, descriptor_id, DynamicPathArguments::empty())
        .unwrap();

    assert_eq!(root.object_id(), HostObjectId(1));
    assert_eq!(root.type_id(), player_id);
    assert_eq!(view.root(), root);
    assert_eq!(view.descriptor_id(), descriptor_id);
    assert_eq!(view.result_type(), i32_id);
    assert_eq!(view.access(), PathAccess::ReadWrite);
    assert!(view.dynamic_args().is_empty());
    assert!(!Value::HostRoot(root).is_storable());
    assert!(!Value::HostPathView(view).is_storable());
}

#[test]
fn validates_dynamic_index_argument_shape_for_path_views() {
    let mut runtime = Runtime::default();
    let i32_id = register_i32(&runtime);
    let item_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(11),
            ..TypeRegistration::new("game.Item", TypeKind::HostPathView)
        })
        .unwrap();
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![
                HostPathSegment::Index {
                    slot: DynamicPathArgSlot::new(0),
                    collection_type: player_id,
                    index_type: i32_id,
                    result_type: item_id,
                    access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(31),
                },
                HostPathSegment::Field {
                    name: "count".to_owned(),
                    field_id: FieldMetadataId::new(1),
                    owner_type: item_id,
                    result_type: i32_id,
                    access: PathAccess::ReadWrite,
                    abi_fingerprint: AbiFingerprint(32),
                },
            ],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(33),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();

    let view = runtime
        .make_host_path_view(
            root,
            descriptor_id,
            DynamicPathArguments::new(vec![DynamicPathArgument::new(i32_id, Value::I32(3))]),
        )
        .unwrap();
    assert_eq!(view.dynamic_args().len(), 1);

    assert_eq!(
        runtime
            .make_host_path_view(root, descriptor_id, DynamicPathArguments::empty())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime
            .make_host_path_view(
                root,
                descriptor_id,
                DynamicPathArguments::new(vec![DynamicPathArgument::new(item_id, Value::I32(3))]),
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );

    let borrow_table = HostBorrowTable::default();
    let frame = borrow_table.enter_frame();
    let borrow = Value::host_ref(
        frame
            .borrow_shared(HostObjectId(99), i32_id)
            .expect("borrow should be created"),
    );
    assert_eq!(
        runtime
            .make_host_path_view(
                root,
                descriptor_id,
                DynamicPathArguments::new(vec![DynamicPathArgument::new(i32_id, borrow)]),
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}

#[test]
fn rejects_roots_and_descriptors_that_exceed_host_path_policy() {
    let mut runtime = Runtime::default();
    let i32_id = register_i32(&runtime);
    let read_only_id = register_host_root_type(&mut runtime, "game.ReadOnly", PathAccess::ReadOnly);
    let opaque_id = register_host_root_type(&mut runtime, "game.Opaque", PathAccess::None);

    assert_eq!(
        runtime
            .register_host_root(HostObjectId(7), opaque_id, HostSchemaEpoch::new(0))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime
            .register_host_path_descriptor(HostPathDescriptorRegistration {
                root_type: read_only_id,
                result_type: i32_id,
                segments: vec![HostPathSegment::Field {
                    name: "hp".to_owned(),
                    field_id: FieldMetadataId::new(0),
                    owner_type: read_only_id,
                    result_type: i32_id,
                    access: PathAccess::ReadOnly,
                    abi_fingerprint: AbiFingerprint(41),
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                abi_fingerprint: AbiFingerprint(42),
                capability_requirements: CapabilitySet::default(),
            })
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}

#[test]
fn rejects_stale_root_metadata_when_creating_views() {
    let mut runtime = Runtime::default();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(51),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(52),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();

    let stale = kagari_runtime::HostRootHandle::new(
        root.object_id(),
        root.type_id(),
        HostSchemaEpoch::new(1),
        root.abi_fingerprint(),
    );

    assert_eq!(
        runtime
            .make_host_path_view(stale, descriptor_id, DynamicPathArguments::empty())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}
