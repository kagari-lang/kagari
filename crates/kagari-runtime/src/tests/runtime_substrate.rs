#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    Runtime,
    gc::GcHeap,
    host::{
        DynamicPathArguments, HostObjectId, HostPathAdapter, HostPathDescriptorId, HostSchemaEpoch,
        PreparedHostPathWrite,
    },
    metadata::{AbiFingerprint, TypeKind, TypeRegistration},
    native::builder::ModuleBuilder,
    value::Value,
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
        .bind_path(
            &path,
            HostPathAdapter::new()
                .with_read(|_, _| Ok(Value::I32(10)))
                .with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {}))),
        )
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
    assert_eq!(runtime.trace_roots().unwrap(), vec![tuple, record, leaf]);
    root.set(runtime.gc(), Value::GcHandle(leaf)).unwrap();
    assert_eq!(runtime.trace_roots().unwrap(), vec![leaf]);
    assert_eq!(root.value(runtime.gc()).unwrap(), Value::GcHandle(leaf));
    drop(root);
    assert!(runtime.trace_roots().unwrap().is_empty());
}

#[test]
fn same_name_loads_have_independent_installations_and_runtime_instances() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = Runtime::default();
    let first_program = load_bytecode(
        &mut runtime,
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
    let second_program = load_bytecode(
        &mut runtime,
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
fn runtime_type_registry_facade_preserves_registered_metadata() {
    let runtime = Runtime::default();
    let player_id = runtime
        .types()
        .register(TypeRegistration {
            abi_fingerprint: AbiFingerprint(5),
            ..TypeRegistration::new("Player", TypeKind::Struct)
        })
        .unwrap();

    assert_eq!(runtime.types().id_by_name("Player"), Some(player_id));
    assert_eq!(runtime.types().get(player_id).unwrap().name, "Player");
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
    assert_eq!(runtime.trace_roots().unwrap(), vec![tuple, script]);
}
