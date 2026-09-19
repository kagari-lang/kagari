//! Prepare a host update and reject a full dirty ledger before touching the field.
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, FieldMetadataId, HostExposurePolicy, HostObjectId,
    HostPathAdapter, HostPathDescriptorRegistration, HostPathSegment, HostSchemaEpoch,
    HostTypeOwnership, HostTypeRegistration, LanguageProfile, PathAccess, ResourcePolicy, Runtime,
    RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeKind, TypeRegistration,
    host::{HostError, PreparedHostPathWrite},
    value::Value,
};
use std::{cell::Cell, rc::Rc};

fn main() {
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_path_mutation: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                path_mutation: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_types: vec!["game.Player".into()],
            allow_host_path_reads: true,
            allow_host_path_mutation: true,
            ..Default::default()
        },
        resources: ResourcePolicy {
            max_dirty_records: Some(1),
            ..Default::default()
        },
        ..Default::default()
    });
    let scalar = runtime
        .types()
        .register(TypeRegistration::new("i32", TypeKind::Primitive))
        .unwrap();
    let mut player = HostTypeRegistration::new(
        kagari_common::host_interface::HostTypeDeclaration::new("game.Player"),
        "Player",
    );
    // Nominal identity can differ from the export label and is shared with
    // offline host signatures; registration does not derive identity from slots.
    player.declaration.id = kagari_common::host_interface::host_type_identity("game.PlayerState");
    player.declaration.ownership = HostTypeOwnership::HostRoot;
    player.declaration.path_access = PathAccess::ReadWrite;

    let player = runtime.register_host_type(player).unwrap();
    let root = runtime
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let path = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player,
            result_type: scalar,
            segments: vec![HostPathSegment::Field {
                name: "hp".into(),
                field_id: FieldMetadataId::new(0),
                owner_type: player,
                result_type: scalar,
                access: PathAccess::ReadWrite,
                abi_fingerprint: AbiFingerprint(2),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(3),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();
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
    let root = Value::HostRoot(root);
    runtime
        .set_host_path(&root, path, vec![], Value::I32(20))
        .unwrap();
    let error = runtime
        .set_host_path(&root, path, vec![], Value::I32(30))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
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
    println!("full ledger preserved hp=20; after draining the ledger, hp=30");
}
