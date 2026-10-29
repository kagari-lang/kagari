use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    error::RuntimeErrorKind,
    gc::GcHeap,
    host::{
        DynamicPathArguments, HostBorrowTable, HostObjectId, HostPathAdapter, HostPathDescriptorId,
        HostSchemaEpoch, PreparedHostPathWrite,
    },
    metadata::{
        AbiFingerprint, FieldInfo, FieldMetadataId, MethodInfo, MethodMetadataId, MethodOrigin,
        ParameterInfo, TraitInfo, TypeId, TypeKind, TypeRegistration,
    },
    native::builder::ModuleBuilder,
    value::{Value, ValueCategory},
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};

use kagari_types::{
    declaration::{
        FieldDef,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
        path::{HostPathDeclaration, HostPathSegmentDeclaration},
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};

use crate::test_support::layouts;

fn host_root_value(heap: &GcHeap, object_id: u64) -> Value {
    let Value::HostPathView(view) = path_view_value(heap, object_id) else {
        unreachable!()
    };
    heap.alloc_host_root(heap.host_path(view).unwrap().root())
        .unwrap()
}

fn path_view_value(heap: &GcHeap, object_id: u64) -> Value {
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    player
        .field(
            DefinitionMetadata::checked(
                DefinitionContext::new().unwrap().snapshot(),
                FieldDef {
                    name: "hp".into(),
                    ty: Ty::Builtin(BuiltinType::I32),
                    visibility: Visibility::Public,
                    mutable: true,
                    host_policy: Some(HostFieldPolicy {
                        readable: true,
                        path_access: PathAccess::ReadWrite,
                    }),
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let player = player.finish().unwrap();
    let hp = player
        .definitions()
        .lookup_child(player.id(), DefinitionKind::Field, "hp", 0)
        .unwrap();
    let path = module
        .define_path(
            DefinitionMetadata::checked(
                player.definitions().clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Field(hp)],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    module
        .bind_path(&path, unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new()
                .with_read_transport(|_, _| Ok(Value::I32(10)))
                .with_prepare_write_transport(|_, _, _| Ok(PreparedHostPathWrite::new(|| {})))
        })
        .unwrap();
    let mut runtime = Runtime::default();
    module.finish().unwrap().install(&mut runtime).unwrap();
    let root_type = runtime.types().get_by_name("game::Player").unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(object_id), root_type, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = HostPathDescriptorId::new(0);
    heap.alloc_host_path(
        runtime
            .host()
            .make_path_view(heap, root, descriptor, DynamicPathArguments::empty())
            .unwrap(),
    )
    .unwrap()
}

fn shared_borrow_value(heap: &GcHeap, object_id: u64) -> Value {
    let table = HostBorrowTable::default();
    let guard = table.enter_frame().unwrap();
    heap.alloc_host_ref(
        guard
            .borrow_shared(HostObjectId(object_id), TypeId::new(0))
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn value_categories_and_storage_boundaries_match_runtime_spec() {
    let runtime = Runtime::default();
    let heap = runtime.gc();
    let host_root = host_root_value(runtime.gc(), 1);
    let host_path_view = path_view_value(runtime.gc(), 2);
    let host_borrow = shared_borrow_value(runtime.gc(), 3);

    assert_eq!(Value::Unit.category(), ValueCategory::Unit);
    assert_eq!(Value::I32(7).category(), ValueCategory::Primitive);
    assert_eq!(
        heap.alloc_tuple(vec![Value::I32(7)]).unwrap().category(),
        ValueCategory::ScriptOwned
    );
    assert_eq!(host_root.category(), ValueCategory::HostHandle);
    assert_eq!(host_path_view.category(), ValueCategory::HostPathView);
    assert_eq!(host_borrow.category(), ValueCategory::Ephemeral);

    assert!(!host_root.is_storable(heap));
    assert!(!host_path_view.is_storable(heap));
    assert!(!host_borrow.is_storable(heap));
    assert!(
        !heap
            .alloc_tuple(vec![host_borrow])
            .unwrap()
            .is_storable(heap)
    );

    assert!(!host_root.is_default_heap_payload(heap));
    assert!(!host_path_view.is_default_heap_payload(heap));
}

#[test]
fn explicit_roots_trace_script_objects_without_crossing_host_boundaries() {
    let mut runtime = Runtime::default();
    let record_layout = layouts::layout(
        &mut runtime,
        "Record",
        &[(
            "leaf",
            Ty::Array(Box::new(Ty::Builtin(
                kagari_types::scalar::BuiltinType::I32,
            ))),
            true,
        )],
    );
    let leaf = runtime
        .alloc_array(
            record_layout.module(),
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    let record = runtime
        .alloc_struct(record_layout, vec![Value::Array(leaf)])
        .unwrap();

    let root = runtime
        .root_value(
            runtime
                .gc()
                .alloc_tuple(vec![Value::Struct(record), Value::Unit])
                .unwrap(),
        )
        .unwrap();

    let Value::Tuple(tuple) = root.value(runtime.gc()).unwrap() else {
        unreachable!()
    };
    assert_eq!(
        runtime.gc().trace_roots().unwrap(),
        vec![tuple, record, leaf]
    );
    root.set(runtime.gc(), Value::GcHandle(leaf)).unwrap();
    assert_eq!(runtime.gc().trace_roots().unwrap(), vec![leaf]);
    assert_eq!(root.value(runtime.gc()).unwrap(), Value::GcHandle(leaf));
    drop(root);
    assert!(runtime.gc().trace_roots().unwrap().is_empty());
}

#[test]
fn same_name_loads_have_independent_installations_and_runtime_instances() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let first_program = runtime
        .load_program(
            "game.player",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let first = runtime.select_program(&first_program).unwrap();
    let second_program = runtime
        .load_program(
            "game.player",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let second = runtime.select_program(&second_program).unwrap();

    assert_ne!(first.id, second.id);
    assert_ne!(first.installation_id(), second.installation_id());
    assert_eq!(first.epoch, second.epoch);
    assert_eq!(first.epoch.0, 1);
    assert_eq!(second.epoch.0, 1);

    let first_instance = runtime.module_instance_snapshot(&first).unwrap();
    let second_instance = runtime.module_instance_snapshot(&second).unwrap();
    assert_eq!(first_instance.epoch, second_instance.epoch);
    assert_ne!(first_instance.id, second_instance.id);
    assert!(first_instance.module_slots.is_empty());
    assert!(second_instance.module_slots.is_empty());
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
    let mut runtime = Runtime::default();

    let record_layout = layouts::layout(
        &mut runtime,
        "HostBacked",
        &[(
            "path",
            Ty::Array(Box::new(Ty::Builtin(
                kagari_types::scalar::BuiltinType::I32,
            ))),
            true,
        )],
    );
    assert!(
        runtime
            .alloc_array(
                record_layout.module(),
                Ty::Builtin(BuiltinType::I32),
                vec![host_root_value(runtime.gc(), 1)]
            )
            .is_err()
    );
    let script = runtime
        .alloc_array(
            record_layout.module(),
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(1)],
        )
        .unwrap();
    assert!(
        runtime
            .alloc_struct(record_layout, vec![path_view_value(runtime.gc(), 2)])
            .is_err()
    );
    assert!(
        runtime
            .root_value(host_root_value(runtime.gc(), 3))
            .is_none()
    );
    assert!(
        runtime
            .root_value(path_view_value(runtime.gc(), 4))
            .is_none()
    );
    let root = runtime
        .root_value(
            runtime
                .gc()
                .alloc_tuple(vec![Value::Array(script), Value::Unit])
                .unwrap(),
        )
        .unwrap();

    let Value::Tuple(tuple) = root.value(runtime.gc()).unwrap() else {
        unreachable!()
    };
    assert_eq!(runtime.gc().trace_roots().unwrap(), vec![tuple, script]);
}
