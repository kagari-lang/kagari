//! Declare and bind manual paths once; prepare updates before committing them.
use kagari_common::identity::{
    DefinitionKind, map::DefinitionContext, metadata::DefinitionMetadata,
};
use kagari_runtime::{
    Runtime,
    error::RuntimeErrorKind,
    host::{HostError, HostObjectId, HostPathAdapter, HostSchemaEpoch, PreparedHostPathWrite},
    native::builder::ModuleBuilder,
    value::Value,
};
use kagari_types::{
    declaration::path::{
        HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration,
    },
    declaration::{
        FieldDef,
        host::{
            HostFieldPolicy, HostReflectionPolicy, HostTypeOwnership, HostTypePolicy, PathAccess,
        },
    },
    scalar::BuiltinType,
    ty::Ty,
    visibility::Visibility,
};
use std::sync::{
    Arc,
    atomic::{AtomicI32, Ordering},
};

fn main() {
    let mut module = ModuleBuilder::new("game::state", &Default::default()).unwrap();
    let field = DefinitionMetadata::checked(
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
    .unwrap();
    let mut player = module.define_type("Player").unwrap();
    player
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Metadata,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    player.field(field).unwrap();
    let player = player.finish().unwrap();
    let definitions = player.definitions();
    let hp_field = definitions
        .lookup_child(player.id(), DefinitionKind::Field, "hp", 0)
        .unwrap();
    let path = module
        .define_path(
            DefinitionMetadata::checked(
                definitions.clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Field(hp_field)],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let preview = module
        .define_path(
            DefinitionMetadata::checked(
                definitions.clone(),
                HostPathDeclaration {
                    root: player.id(),
                    segments: vec![HostPathSegmentDeclaration::Virtual(
                        HostVirtualSegmentDeclaration {
                            name: "hp_preview".into(),
                            result: Ty::Builtin(BuiltinType::I32),
                            access: PathAccess::ReadOnly,
                        },
                    )],
                    access: PathAccess::ReadOnly,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let hp = Arc::new(AtomicI32::new(10));
    let (read_hp, prepare_hp) = (hp.clone(), hp.clone());
    module
        .bind_path(&path, unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new()
                .with_read_transport(move |_, _| Ok(Value::I32(read_hp.load(Ordering::SeqCst))))
                .with_prepare_write_transport(move |_, _, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    if value < 0 {
                        return Err(HostError::new("hp cannot be negative"));
                    }
                    // Capture the checked location and value. Preparation does not mutate it.
                    let hp = prepare_hp.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        hp.store(value, Ordering::SeqCst)
                    }))
                })
        })
        .unwrap();
    let preview_hp = hp.clone();
    module
        .bind_path(&preview, unsafe {
            // SAFETY: Path inputs remain scoped; providers retain escaping edges and publish results immediately.
            HostPathAdapter::new()
                .with_read_transport(move |_, _| Ok(Value::I32(preview_hp.load(Ordering::SeqCst))))
        })
        .unwrap();
    let mut runtime = Runtime::default();
    module.finish().unwrap().install(&mut runtime).unwrap();
    let player = runtime
        .types()
        .get_by_name("game::state::Player")
        .unwrap()
        .id;
    let root = runtime
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let path = runtime.resolve_host_path(&path).unwrap();
    let preview = runtime.resolve_host_path(&preview).unwrap();
    let root = unsafe { runtime.gc().alloc_host_root_transport(root) }.unwrap();
    assert_eq!(
        unsafe { runtime.read_host_path_transport(&root, preview, vec![]) }.unwrap(),
        Value::I32(10)
    );
    unsafe { runtime.set_host_path_transport(&root, path, vec![], Value::I32(20)) }.unwrap();
    let error = unsafe { runtime.set_host_path_transport(&root, preview, vec![], Value::I32(30)) }
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
    assert_eq!(hp.load(Ordering::SeqCst), 20);
    assert_eq!(unsafe { runtime.host_dirty_paths_transport() }.len(), 1);
    assert!(!runtime.is_quarantined());
    let record = unsafe { runtime.host_dirty_paths_transport() }.remove(0);
    assert_eq!(record.old_value, Some(Value::I32(10)));
    assert_eq!(record.new_value, Value::I32(20));
    runtime.clear_host_dirty_paths();
    unsafe { runtime.set_host_path_transport(&root, path, vec![], Value::I32(30)) }.unwrap();
    assert_eq!(hp.load(Ordering::SeqCst), 30);
    println!("readonly view preserved hp=20; writable path updated hp=30");
}
