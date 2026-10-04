//! Prepare a host update and reject a full dirty ledger before touching the field.

use kagari_types::{
    host_interface,
    host_interface::{
        path::{HostPathDeclaration, HostPathSegmentDeclaration, HostVirtualSegmentDeclaration},
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    },
};

use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        HostError, HostObjectId, HostPathAdapter, HostPathDescriptorRegistration,
        HostPathSegmentRegistration, HostSchemaEpoch, HostTypeRegistration, PreparedHostPathWrite,
    },
    resource::RuntimeLimits,
    value::Value,
};
use kagari_types::host_interface::type_declaration::{HostTypeOwnership, PathAccess};
use std::{cell::Cell, rc::Rc};

fn main() {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    let mut player = HostTypeRegistration::new(HostTypeDeclaration::new("game.Player"), "Player");
    // Nominal identity can differ from the export label and is shared with
    // offline host signatures; registration does not derive identity from slots.
    player.declaration.id = host_interface::host_type_identity("game.PlayerState");
    player.declaration.ownership = HostTypeOwnership::HostRoot;
    player.declaration.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(&player.declaration.id, "hp", HostValueType::I32);
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    let hp_declaration = hp.id.clone();
    player.declaration.fields.push(hp);

    let path_declaration = HostPathDeclaration {
        root: player.declaration.id.clone(),
        segments: vec![HostPathSegmentDeclaration::Field(hp_declaration)],
        access: PathAccess::ReadWrite,
        schema_epoch: 0,
    };
    let player = runtime.register_host_type(player).unwrap();
    let scalar = runtime.types().get(player).unwrap().fields[0].ty;
    let root = runtime
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let path = runtime.register_host_path(&path_declaration).unwrap();
    let hp = Rc::new(Cell::new(10));
    let read_hp = hp.clone();
    let prepare_hp = hp.clone();
    runtime
        .register_host_path_adapter(
            path,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(Value::I32(read_hp.get())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    if value < 0 {
                        return Err(HostError::new("hp cannot be negative"));
                    }
                    // Capture the resolved host location and checked scalar. No target write yet.
                    let hp = prepare_hp.clone();
                    Ok(PreparedHostPathWrite::new(move || hp.set(value)))
                }),
        )
        .unwrap();
    let preview = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player,
            result_type: scalar,
            segments: vec![HostPathSegmentRegistration::Virtual {
                declaration: HostVirtualSegmentDeclaration {
                    name: "hp_preview".into(),
                    result: HostValueType::I32,
                    access: PathAccess::ReadOnly,
                },
            }],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap();
    let preview_hp = hp.clone();
    runtime
        .register_host_path_adapter(
            preview,
            HostPathAdapter::new().with_read(move |_, _| Ok(Value::I32(preview_hp.get()))),
        )
        .unwrap();
    let root = Value::HostRoot(root);
    assert_eq!(
        runtime.read_host_path(&root, preview, vec![]).unwrap(),
        Value::I32(10)
    );
    runtime
        .set_host_path(&root, path, vec![], Value::I32(20))
        .unwrap();
    let error = runtime
        .set_host_path(&root, preview, vec![], Value::I32(30))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
    assert_eq!(hp.get(), 20);
    assert_eq!(runtime.host_dirty_paths().len(), 1);
    assert!(!runtime.is_quarantined());
    // The host consumes committed records outside the mutation action.
    let record = runtime.host_dirty_paths().remove(0);
    assert_eq!(record.old_value, Some(Value::I32(10)));
    assert_eq!(record.new_value, Value::I32(20));
    runtime.clear_host_dirty_paths();
    runtime
        .set_host_path(&root, path, vec![], Value::I32(30))
        .unwrap();
    assert_eq!(hp.get(), 30);
    println!("readonly view preserved hp=20; writable path updated hp=30");
}
