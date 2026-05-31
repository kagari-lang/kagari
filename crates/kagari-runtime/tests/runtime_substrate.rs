use kagari_ir::bytecode::BytecodeModule;
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldInfo, FieldMetadataId, MethodInfo, MethodMetadataId,
    MethodOrigin, ModuleInitializationState, ParameterInfo, PathAccess, Runtime, RuntimeErrorKind,
    TraitInfo, TypeId, TypeKind, TypeRegistration, Visibility,
    host::{
        DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathDescriptorRegistration,
        HostPathSegment, HostRegistry, HostRootHandle, HostSchemaEpoch, HostTypeInfo,
        HostTypeOwnership,
    },
    value::{StructValueField, Value, ValueCategory},
};

fn host_root_value(object_id: u64) -> Value {
    Value::HostRoot(HostRootHandle::new(
        HostObjectId(object_id),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ))
}

fn path_view_value(object_id: u64) -> Value {
    let root_type = TypeId::new(0);
    let result_type = TypeId::new(1);
    let mut registry = HostRegistry::default();
    registry
        .register_type(HostTypeInfo {
            type_id: root_type,
            script_name: "Player".to_owned(),
            rust_type_name: "Player".to_owned(),
            ownership: HostTypeOwnership::HostRoot,
            fields: Vec::new(),
            methods: Vec::new(),
            traits: Vec::new(),
            path_access: PathAccess::ReadWrite,
            reflection: kagari_runtime::host::HostReflectionPolicy::Hidden,
            abi_fingerprint: AbiFingerprint(1),
        })
        .unwrap();
    let root = registry
        .register_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = registry
        .register_path_descriptor(HostPathDescriptorRegistration {
            root_type,
            result_type,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: root_type,
                result_type,
                access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(2),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(3),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();
    Value::HostPathView(
        registry
            .make_path_view(root, descriptor, DynamicPathArguments::empty())
            .unwrap(),
    )
}

fn shared_borrow_value(object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame();
    Value::host_ref(
        guard
            .borrow_shared(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
}

#[test]
fn value_categories_and_storage_boundaries_match_runtime_spec() {
    let host_root = host_root_value(1);
    let host_path_view = path_view_value(2);
    let host_borrow = shared_borrow_value(3);

    assert_eq!(Value::Unit.category(), ValueCategory::Unit);
    assert_eq!(Value::I32(7).category(), ValueCategory::Primitive);
    assert_eq!(
        Value::Tuple(vec![Value::I32(7)]).category(),
        ValueCategory::ScriptOwned
    );
    assert_eq!(host_root.category(), ValueCategory::HostHandle);
    assert_eq!(host_path_view.category(), ValueCategory::HostPathView);
    assert_eq!(host_borrow.category(), ValueCategory::Ephemeral);

    assert!(!host_root.is_storable());
    assert!(!host_path_view.is_storable());
    assert!(!host_borrow.is_storable());
    assert!(!Value::Tuple(vec![host_borrow]).is_storable());

    assert!(!host_root.is_default_heap_payload());
    assert!(!host_path_view.is_default_heap_payload());
}

#[test]
fn explicit_roots_trace_script_objects_without_crossing_host_boundaries() {
    let runtime = Runtime::default();
    let leaf = runtime.gc().alloc_array(vec![Value::I32(1)]).unwrap();
    let record = runtime
        .gc()
        .alloc_struct(
            "Record".to_owned(),
            vec![StructValueField {
                name: "leaf".to_owned(),
                value: Value::Array(leaf),
            }],
        )
        .unwrap();

    let root = runtime
        .root_value(Value::Tuple(vec![Value::Struct(record), Value::Unit]))
        .unwrap();

    assert_eq!(runtime.trace_roots(), vec![record, leaf]);
    runtime.update_root(root, Value::GcHandle(leaf)).unwrap();
    assert_eq!(runtime.trace_roots(), vec![leaf]);
    assert_eq!(runtime.release_root(root), Some(Value::GcHandle(leaf)));
    assert!(runtime.trace_roots().is_empty());
}

#[test]
fn module_epochs_and_initialization_state_live_in_runtime_store() {
    let mut runtime = Runtime::default();
    let first = runtime
        .load_module("game.player", BytecodeModule::default())
        .unwrap();
    let second = runtime
        .load_module("game.player", BytecodeModule::default())
        .unwrap();

    assert_eq!(first.id, second.id);
    assert_ne!(first.epoch, second.epoch);
    assert_eq!(first.epoch.0, 1);
    assert_eq!(second.epoch.0, 2);

    let first_instance = runtime.module_instance_snapshot(&first).unwrap();
    assert_eq!(
        first_instance.state,
        ModuleInitializationState::Uninitialized
    );
    assert_eq!(first_instance.init_result, None);

    {
        let mut instance = runtime.module_instance_mut(&second).unwrap();
        instance.begin_initialization();
        instance.finish_initialization(Value::I32(42));
    }
    let second_instance = runtime.module_instance_snapshot(&second).unwrap();
    assert_eq!(
        second_instance.state,
        ModuleInitializationState::Initialized
    );
    assert_eq!(second_instance.init_result, Some(Value::I32(42)));
}

#[test]
fn metadata_registry_carries_reload_and_path_validation_records() {
    let runtime = Runtime::default();
    let i32_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(1),
            ..TypeRegistration::new("i32", TypeKind::Primitive)
        })
        .unwrap();
    let trait_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(2),
            ..TypeRegistration::new("Damageable", TypeKind::Interface)
        })
        .unwrap();

    let player_id = runtime
        .types()
        .register(TypeRegistration {
            fields: vec![FieldInfo {
                id: FieldMetadataId::new(0),
                name: "health".to_owned(),
                ty: i32_id,
                readable: true,
                writable: true,
                visibility: Visibility::Public,
                path_access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(3),
            }],
            methods: vec![MethodInfo {
                id: MethodMetadataId::new(0),
                name: "damage".to_owned(),
                params: vec![ParameterInfo {
                    name: "amount".to_owned(),
                    ty: i32_id,
                }],
                return_type: i32_id,
                origin: MethodOrigin::Trait(trait_id),
                capability_requirements: CapabilitySet::default(),
                abi_fingerprint: AbiFingerprint(4),
            }],
            traits: vec![TraitInfo {
                trait_type: trait_id,
                name: "Damageable".to_owned(),
                abi_fingerprint: AbiFingerprint(2),
            }],
            abi_fingerprint: AbiFingerprint(5),
            ..TypeRegistration::new("Player", TypeKind::Struct)
        })
        .unwrap();

    let player = runtime.types().get(player_id).unwrap();
    assert_eq!(runtime.types().id_by_name("Player"), Some(player_id));
    assert_eq!(player.fields[0].path_access, PathAccess::ReadWrite);
    assert_eq!(player.methods[0].origin, MethodOrigin::Trait(trait_id));
    assert_eq!(player.traits[0].trait_type, trait_id);
    assert!(
        runtime
            .types()
            .public_abi_fingerprints()
            .contains(&AbiFingerprint(5))
    );

    let duplicate = runtime
        .types()
        .register(TypeRegistration::new("Player", TypeKind::Struct))
        .unwrap_err();
    assert_eq!(duplicate.kind(), RuntimeErrorKind::MetadataConflict);
}

#[test]
fn host_objects_are_not_gc_payloads_or_trace_targets() {
    let runtime = Runtime::default();

    assert!(runtime.gc().alloc_array(vec![host_root_value(1)]).is_none());
    assert!(
        runtime
            .gc()
            .alloc_struct(
                "HostBacked".to_owned(),
                vec![StructValueField {
                    name: "path".to_owned(),
                    value: path_view_value(2),
                }],
            )
            .is_none()
    );

    let script = runtime.gc().alloc_array(vec![Value::I32(1)]).unwrap();
    assert!(runtime.root_value(host_root_value(3)).is_none());
    assert!(runtime.root_value(path_view_value(4)).is_none());
    runtime
        .root_value(Value::Tuple(vec![Value::Array(script), Value::Unit]))
        .unwrap();

    assert_eq!(runtime.trace_roots(), vec![script]);
}
