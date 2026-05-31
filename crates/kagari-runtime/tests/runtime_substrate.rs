use kagari_ir::bytecode::BytecodeModule;
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldInfo, FieldMetadataId, MethodInfo, MethodMetadataId,
    MethodOrigin, ModuleInitializationState, ParameterInfo, PathAccess, Runtime, RuntimeErrorKind,
    TraitInfo, TypeKind, TypeRegistration, Visibility,
    host::HostObjectId,
    value::{HostPathViewId, StructValueField, Value, ValueCategory},
};

#[test]
fn value_categories_and_storage_boundaries_match_runtime_spec() {
    let host_owned = Value::HostOwned(HostObjectId(1));
    let host_path_view = Value::HostPathView(HostPathViewId(2));
    let host_borrow = Value::host_ref(HostObjectId(3));

    assert_eq!(Value::Unit.category(), ValueCategory::Unit);
    assert_eq!(Value::I32(7).category(), ValueCategory::Primitive);
    assert_eq!(
        Value::Tuple(vec![Value::I32(7)]).category(),
        ValueCategory::ScriptOwned
    );
    assert_eq!(host_owned.category(), ValueCategory::HostHandle);
    assert_eq!(host_path_view.category(), ValueCategory::HostPathView);
    assert_eq!(host_borrow.category(), ValueCategory::Ephemeral);

    assert!(host_owned.is_storable());
    assert!(host_path_view.is_storable());
    assert!(!host_borrow.is_storable());
    assert!(!Value::Tuple(vec![host_borrow]).is_storable());

    assert!(!host_owned.is_default_heap_payload());
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
        .root_value(Value::Tuple(vec![
            Value::Struct(record),
            Value::HostOwned(HostObjectId(10)),
            Value::HostPathView(HostPathViewId(11)),
        ]))
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

    assert!(
        runtime
            .gc()
            .alloc_array(vec![Value::HostOwned(HostObjectId(1))])
            .is_none()
    );
    assert!(
        runtime
            .gc()
            .alloc_struct(
                "HostBacked".to_owned(),
                vec![StructValueField {
                    name: "path".to_owned(),
                    value: Value::HostPathView(HostPathViewId(2)),
                }],
            )
            .is_none()
    );

    let script = runtime.gc().alloc_array(vec![Value::I32(1)]).unwrap();
    runtime
        .root_value(Value::Tuple(vec![
            Value::HostOwned(HostObjectId(3)),
            Value::HostPathView(HostPathViewId(4)),
            Value::Array(script),
        ]))
        .unwrap();

    assert_eq!(runtime.trace_roots(), vec![script]);
}
