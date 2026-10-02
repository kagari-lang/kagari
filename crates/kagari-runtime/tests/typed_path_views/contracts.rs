use super::*;

#[test]
fn index_and_virtual_path_fingerprints_follow_resolved_contracts() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let index = |access| HostPathDescriptorRegistration {
        root_type: owner,
        result_type: scalar,
        segments: vec![HostPathSegmentRegistration::Index {
            declaration: HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::opaque("game.Player"),
                index: HostValueType::I32,
                result: HostValueType::I32,
                access,
            },
        }],
        access,
        schema_epoch: HostSchemaEpoch::new(0),
        capability_requirements: CapabilitySet::default(),
    };
    let first = runtime
        .register_host_path_descriptor(index(PathAccess::ReadOnly))
        .unwrap();
    let repeated = runtime
        .register_host_path_descriptor(index(PathAccess::ReadOnly))
        .unwrap();
    let writable = runtime
        .register_host_path_descriptor(index(PathAccess::ReadWrite))
        .unwrap();
    let fingerprint =
        |runtime: &Runtime, id| runtime.host().path_descriptor(id).unwrap().abi_fingerprint;
    assert_eq!(
        fingerprint(&runtime, first),
        fingerprint(&runtime, repeated)
    );
    assert_ne!(
        fingerprint(&runtime, first),
        fingerprint(&runtime, writable)
    );
    assert_eq!(
        runtime.host().path_descriptor(first).unwrap().segments[0].abi_fingerprint(),
        AbiFingerprint(0)
    );

    let virtual_path = |name: &str| HostPathDescriptorRegistration {
        root_type: owner,
        result_type: scalar,
        segments: vec![HostPathSegmentRegistration::Virtual {
            declaration: HostVirtualSegmentDeclaration {
                name: name.into(),
                result: HostValueType::I32,
                access: PathAccess::ReadOnly,
            },
        }],
        access: PathAccess::ReadOnly,
        schema_epoch: HostSchemaEpoch::new(0),
        capability_requirements: CapabilitySet::default(),
    };
    let health = runtime
        .register_host_path_descriptor(virtual_path("health"))
        .unwrap();
    let mana = runtime
        .register_host_path_descriptor(virtual_path("mana"))
        .unwrap();
    assert_ne!(fingerprint(&runtime, health), fingerprint(&runtime, mana));
    assert_eq!(
        runtime.host().path_descriptor(health).unwrap().segments[0].abi_fingerprint(),
        AbiFingerprint(0)
    );

    let mut shifted = path_mutation_runtime();
    let shifted_scalar = register_i32(&shifted);
    register_host_root_type(&mut shifted, "game.Opaque", PathAccess::ReadOnly);
    let shifted_owner = register_host_root_type(&mut shifted, "game.Player", PathAccess::ReadWrite);
    let shifted_index = shifted
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: shifted_owner,
            result_type: shifted_scalar,
            ..index(PathAccess::ReadOnly)
        })
        .unwrap();
    assert_ne!(owner, shifted_owner);
    assert_eq!(
        fingerprint(&runtime, first),
        fingerprint(&shifted, shifted_index)
    );

    let offline = runtime.host().interface();
    let encoded = offline.to_bytes().unwrap();
    let offline = kagari_common::host_interface::HostInterface::from_bytes(&encoded).unwrap();
    assert_eq!(offline.paths.len(), 4);
    let mut bound = path_mutation_runtime();
    register_i32(&bound);
    register_host_root_type(&mut bound, "game.Player", PathAccess::ReadWrite);
    for path in &offline.paths {
        let id = bound.register_host_path(path).unwrap();
        assert_eq!(
            bound.host().path_descriptor(id).unwrap().abi_fingerprint.0,
            path.contract(&offline).unwrap().fingerprint().unwrap()
        );
    }
    bound.host().link_interface(&offline).unwrap();
}

#[test]
fn portable_path_segments_reject_unbound_types_before_publication() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    for segment in [
        HostPathSegmentRegistration::Index {
            declaration: HostIndexSegmentDeclaration {
                slot: 0,
                collection: HostValueType::opaque("game.Player"),
                index: HostValueType::opaque("game.Missing"),
                result: HostValueType::I32,
                access: PathAccess::ReadOnly,
            },
        },
        HostPathSegmentRegistration::Virtual {
            declaration: HostVirtualSegmentDeclaration {
                name: "missing".into(),
                result: HostValueType::opaque("game.Missing"),
                access: PathAccess::ReadOnly,
            },
        },
    ] {
        assert_eq!(
            runtime
                .register_host_path_descriptor(HostPathDescriptorRegistration {
                    root_type: owner,
                    result_type: scalar,
                    segments: vec![segment],
                    access: PathAccess::ReadOnly,
                    schema_epoch: HostSchemaEpoch::new(0),
                    capability_requirements: CapabilitySet::default(),
                })
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::TypedPathValidation
        );
        assert_eq!(runtime.host().path_descriptors().count(), 0);
    }
}

#[test]
fn heap_path_temporaries_survive_collection_during_write_preparation() {
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    let mut runtime = path_mutation_runtime();
    use kagari_common::host_interface::{
        type_declaration::{HostFieldDeclaration, HostTypeDeclaration},
        value_type::HostValueType,
    };
    let mut declaration = HostTypeDeclaration::new("game.Player");
    declaration.ownership = HostTypeOwnership::HostRoot;
    declaration.path_access = PathAccess::ReadWrite;
    let mut hp = HostFieldDeclaration::new(
        &declaration.id,
        "hp",
        HostValueType::Array(Box::new(HostValueType::I32), CollectionAccess::Mutable),
    );
    hp.writable = true;
    hp.path_access = PathAccess::ReadWrite;
    declaration.fields.push(hp);
    let player_type = runtime
        .register_host_type(HostTypeRegistration::new(declaration, "Player"))
        .unwrap();
    let result_type = runtime.types().get(player_type).unwrap().fields[0].ty;
    let root = runtime
        .register_host_root(HostObjectId(1), player_type, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(
        &mut runtime,
        player_type,
        result_type,
        PathAccess::ReadWrite,
    );
    let allocation = allocation_owner(&mut runtime);
    let read_allocation = allocation.clone();
    let access = Rc::new(RefCell::new(None::<Weak<Runtime>>));
    let previous = Rc::new(Cell::new(None));
    let read_access = access.clone();
    let read_previous = previous.clone();
    let write_access = access.clone();
    let write_previous = previous.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| {
                    let runtime = read_access.borrow().as_ref().unwrap().upgrade().unwrap();
                    let value = runtime
                        .alloc_array(
                            &read_allocation,
                            AbiType::Builtin(BuiltinType::I32),
                            vec![Value::I32(1)],
                        )
                        .unwrap();
                    read_previous.set(Some(value));
                    Ok(Value::Array(value))
                })
                .with_prepare_write(move |_, _, record| {
                    let value = record.new_value.clone();
                    let runtime = write_access.borrow().as_ref().unwrap().upgrade().unwrap();
                    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
                    assert_eq!(
                        runtime.gc().array_get(write_previous.get().unwrap(), 0),
                        Some(Value::I32(1))
                    );
                    let Value::Array(next) = value else {
                        panic!("array path value")
                    };
                    assert_eq!(runtime.gc().array_get(next, 0), Some(Value::I32(2)));
                    Ok(PreparedHostPathWrite::new(|| {}))
                }),
        )
        .unwrap();
    let runtime = Rc::new(runtime);
    *access.borrow_mut() = Some(Rc::downgrade(&runtime));
    let next = runtime
        .alloc_array(
            &allocation,
            AbiType::Builtin(BuiltinType::I32),
            vec![Value::I32(2)],
        )
        .unwrap();
    runtime
        .set_host_path(
            &Value::HostRoot(root),
            descriptor,
            vec![],
            Value::Array(next),
        )
        .unwrap();
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 2);
    runtime.clear_host_dirty_paths();
    assert_eq!(runtime.collect_garbage().unwrap().reclaimed_objects, 2);
    let mut foreign = Runtime::default();
    let foreign_owner = allocation_owner(&mut foreign);
    let other = foreign
        .alloc_array(&foreign_owner, AbiType::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert!(
        runtime
            .set_host_path(
                &Value::HostRoot(root),
                descriptor,
                vec![],
                Value::Array(other)
            )
            .is_err()
    );
    assert!(runtime.host_dirty_paths().is_empty());
    assert_eq!(runtime.gc().allocated_objects(), 0);
}

#[test]
fn rejects_disconnected_path_types_before_publishing_descriptors() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let player = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let field = |owner_type| HostPathSegmentRegistration::Field {
        declaration: HostFieldDeclaration::new(
            &kagari_common::host_interface::host_type_identity(if owner_type == player {
                "game.Player"
            } else {
                "game.Other"
            }),
            "hp",
            HostValueType::I32,
        )
        .id,
    };
    let index = |collection_type| HostPathSegmentRegistration::Index {
        declaration: HostIndexSegmentDeclaration {
            slot: 0,
            collection: if collection_type == player {
                HostValueType::opaque("game.Player")
            } else {
                HostValueType::I32
            },
            index: HostValueType::I32,
            result: HostValueType::I32,
            access: PathAccess::ReadWrite,
        },
    };
    for segments in [
        vec![field(scalar)],
        vec![index(scalar)],
        vec![field(player), field(player)],
        vec![field(player), index(player)],
    ] {
        let result = runtime.register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player,
            result_type: scalar,
            segments,
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements: CapabilitySet::default(),
        });
        assert_eq!(
            result.unwrap_err().kind(),
            RuntimeErrorKind::TypedPathValidation
        );
    }
    // Failed registrations neither publish descriptors nor consume their slots.
    let actual = register_hp_descriptor(&mut runtime, player, scalar, PathAccess::ReadWrite);
    let mut fresh = path_mutation_runtime();
    let fresh_scalar = register_i32(&fresh);
    let fresh_player = register_host_root_type(&mut fresh, "game.Player", PathAccess::ReadWrite);
    let expected = register_hp_descriptor(
        &mut fresh,
        fresh_player,
        fresh_scalar,
        PathAccess::ReadWrite,
    );
    assert_eq!(actual, expected);
}

#[test]
fn registers_typed_host_roots_and_simple_path_views() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();

    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: runtime
                    .host()
                    .host_type(player_id)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "hp")
                    .unwrap()
                    .id
                    .clone(),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let item_id = register_host_root_type(&mut runtime, "game.Item", PathAccess::ReadWrite);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![
                HostPathSegmentRegistration::Index {
                    declaration: HostIndexSegmentDeclaration {
                        slot: 0,
                        collection: HostValueType::opaque("game.Player"),
                        index: HostValueType::I32,
                        result: HostValueType::opaque("game.Item"),
                        access: PathAccess::ReadWrite,
                    },
                },
                HostPathSegmentRegistration::Field {
                    declaration: runtime
                        .host()
                        .host_type(item_id)
                        .unwrap()
                        .declaration
                        .fields
                        .iter()
                        .find(|field| field.name == "count")
                        .unwrap()
                        .id
                        .clone(),
                },
            ],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
    let frame = borrow_table.enter_frame().unwrap();
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
fn host_paths_are_unavailable_until_exposed() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_read = Arc::clone(&calls);
    let mut runtime = Runtime::default();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id =
        register_hp_descriptor(&mut runtime, player_id, i32_id, PathAccess::ReadOnly);
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new().with_read(move |_, _| {
                *calls_for_read.lock().expect("read counter should lock") += 1;
                Ok(Value::I32(7))
            }),
        )
        .unwrap();

    let error = runtime
        .read_host_path(&Value::HostRoot(root), descriptor_id, Vec::new())
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::CapabilityDenied);
    assert_eq!(*calls.lock().expect("read counter should lock"), 0);

    runtime.set_host_exposure_policy(HostExposurePolicy {
        allowed_host_types: vec!["game.Player".to_owned()],
        allow_host_path_reads: true,
        ..HostExposurePolicy::default()
    });

    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor_id, Vec::new())
            .unwrap(),
        Value::I32(7)
    );
    assert_eq!(*calls.lock().expect("read counter should lock"), 1);
}

#[test]
fn path_execution_validates_stale_roots_and_dynamic_indexes() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentRegistration::Index {
                declaration: HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: HostValueType::opaque("game.Player"),
                    index: HostValueType::I32,
                    result: HostValueType::I32,
                    access: PathAccess::ReadOnly,
                },
            }],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();

    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new().with_read(|_, context| {
                let Value::I32(index) = &context.dynamic_args.as_slice()[0].value else {
                    return Err(HostError::new("inventory index must be i32"));
                };
                Ok(Value::I32(*index * 10))
            }),
        )
        .unwrap();

    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor_id, vec![Value::I32(4)])
            .unwrap(),
        Value::I32(40)
    );
    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor_id, Vec::new())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );

    let stale = runtime
        .register_host_root(HostObjectId(2), root.type_id(), HostSchemaEpoch::new(1))
        .unwrap();
    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(stale), descriptor_id, vec![Value::I32(1)])
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
                segments: vec![HostPathSegmentRegistration::Field {
                    declaration: runtime
                        .host()
                        .host_type(read_only_id)
                        .unwrap()
                        .declaration
                        .fields
                        .iter()
                        .find(|field| field.name == "hp")
                        .unwrap()
                        .id
                        .clone()
                }],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
                capability_requirements: CapabilitySet::default(),
            })
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}

#[test]
fn rejects_root_schema_mismatch_when_creating_views() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: runtime
                    .host()
                    .host_type(player_id)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "hp")
                    .unwrap()
                    .id
                    .clone(),
            }],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap();

    let stale = runtime
        .register_host_root(HostObjectId(2), root.type_id(), HostSchemaEpoch::new(1))
        .unwrap();

    assert_eq!(
        runtime
            .make_host_path_view(stale, descriptor_id, DynamicPathArguments::empty())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}
