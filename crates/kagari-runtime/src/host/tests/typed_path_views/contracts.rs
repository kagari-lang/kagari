use super::*;
use kagari_bytecode::artifact::{ArtifactBuildOptions, KbcArtifact};
use kagari_contract::types::PublicItem;
use kagari_types::declaration::{TypeDeclaration, path::HostPathDeclaration};
use std::collections::BTreeMap;

#[test]
fn index_and_virtual_path_fingerprints_follow_resolved_contracts() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let definitions = runtime.host().definitions();
    let player = host_declaration(&runtime, owner);
    let index = |access| HostPathDescriptorRegistration {
        definitions: definitions.clone(),
        root_type: owner,
        result_type: scalar,
        segments: vec![HostPathSegmentDeclaration::Index(
            HostIndexSegmentDeclaration {
                slot: 0,
                collection: Ty::Host(player),
                index: Ty::Builtin(BuiltinType::I32),
                result: Ty::Builtin(BuiltinType::I32),
                access,
            },
        )],
        access,
        schema_epoch: HostSchemaEpoch::new(0),
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
        definitions: definitions.clone(),
        root_type: owner,
        result_type: scalar,
        segments: vec![HostPathSegmentDeclaration::Virtual(
            HostVirtualSegmentDeclaration {
                name: name.into(),
                result: Ty::Builtin(BuiltinType::I32),
                access: PathAccess::ReadOnly,
            },
        )],
        access: PathAccess::ReadOnly,
        schema_epoch: HostSchemaEpoch::new(0),
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
    register_host_root_type(&mut shifted, "game::Opaque", PathAccess::ReadOnly);
    let shifted_owner =
        register_host_root_type(&mut shifted, "game::Player", PathAccess::ReadWrite);
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

    // The cold artifact owns the original schemas and four deduplicated contracts.
    let owner = root_module(
        "game::Player",
        PathAccess::ReadWrite,
        ["hp", "secure_hp", "count"]
            .into_iter()
            .map(|name| (name, Ty::Builtin(BuiltinType::I32)))
            .collect(),
    );
    let mut builder = ModuleBuilder::from_declaration(
        owner.to_declaration(),
        &Default::default(),
        BTreeMap::new(),
    )
    .unwrap();
    for id in [first, writable, health, mana] {
        let path = builder
            .define_path(
                DefinitionMetadata::checked(
                    runtime.host().definitions(),
                    runtime
                        .host()
                        .path_descriptor(id)
                        .unwrap()
                        .declaration
                        .clone(),
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
        let mut adapter = HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(7)));
        if path.records().access == PathAccess::ReadWrite {
            adapter = adapter.with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {})));
        }
        builder.bind_path(&path, adapter).unwrap();
    }
    let bound_module = builder.finish().unwrap();
    let context = DefinitionContext::new().unwrap();
    let declaration = bound_module
        .to_declaration()
        .import_into(&context, &Default::default())
        .unwrap();
    let records = declaration.records();
    assert_eq!(records.paths.len(), 4);
    let schema = TypeDeclaration {
        declaration: records
            .definition(DefinitionKind::Struct, "Player", declaration.definitions())
            .unwrap(),
        definition: records.types[0].clone(),
    };
    let artifact = KbcArtifact::from_program(
        BytecodeProgram {
            root: ModuleRef::new(0),
            exports: Default::default(),
            modules: vec![BytecodeModule {
                identity: records.identity.clone(),
                external_types: vec![schema],
                path_declarations: records.paths.clone(),
                public_items: records
                    .types
                    .iter()
                    .cloned()
                    .map(PublicItem::Type)
                    .collect(),
                ..Default::default()
            }],
        },
        ArtifactBuildOptions::default(),
        &context,
        &Default::default(),
    )
    .unwrap();
    let encoded = artifact.to_bytes(&context.snapshot()).unwrap();
    let offline = KbcArtifact::from_bytes(&encoded).unwrap();
    let decode_context = DefinitionContext::new().unwrap();
    let offline = offline
        .import_into(&decode_context, &Default::default())
        .unwrap();
    offline
        .records()
        .validate_for_loader(&Default::default(), &decode_context, &Default::default())
        .unwrap();
    let mut bound = path_mutation_runtime();
    bound_module.install(&mut bound).unwrap();
    let required = &offline.records().program.modules[0];
    for (path, original) in required
        .path_declarations
        .iter()
        .zip([first, writable, health, mana])
    {
        let path = DefinitionMetadata::checked(
            offline.definitions().clone(),
            path.clone(),
            &Default::default(),
        )
        .unwrap();
        let id = bound.resolve_host_path(&path).unwrap();
        assert_eq!(
            bound.host().path_descriptor(id).unwrap().abi_fingerprint,
            fingerprint(&runtime, original)
        );
    }
    bound
        .load_program(
            "cold-paths",
            DefinitionMetadata::checked(
                offline.definitions().clone(),
                offline.records().program.clone(),
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn portable_path_segments_reject_unbound_types_before_publication() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let context = DefinitionContext::new().unwrap();
    let player = nominal(&context, "game::Player");
    let missing = nominal(&context, "game::Missing");
    for segment in [
        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
            slot: 0,
            collection: Ty::Host(player),
            index: Ty::Host(missing),
            result: Ty::Builtin(BuiltinType::I32),
            access: PathAccess::ReadOnly,
        }),
        HostPathSegmentDeclaration::Virtual(HostVirtualSegmentDeclaration {
            name: "missing".into(),
            result: Ty::Host(missing),
            access: PathAccess::ReadOnly,
        }),
    ] {
        assert_eq!(
            runtime
                .register_host_path_descriptor(HostPathDescriptorRegistration {
                    definitions: context.snapshot(),
                    root_type: owner,
                    result_type: scalar,
                    segments: vec![segment],
                    access: PathAccess::ReadOnly,
                    schema_epoch: HostSchemaEpoch::new(0),
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
    let mut runtime = path_mutation_runtime();
    root_module(
        "game::Player",
        PathAccess::ReadWrite,
        vec![("hp", Ty::Array(Box::new(Ty::Builtin(BuiltinType::I32))))],
    )
    .install(&mut runtime)
    .unwrap();
    let player_type = runtime.types().get_by_name("game::Player").unwrap().id;
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
    let previous = Arc::new(Mutex::new(None));
    let read_previous = previous.clone();
    let write_previous = previous.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |cx, _| {
                    let runtime = cx.runtime();
                    let value = runtime
                        .alloc_array(
                            &read_allocation,
                            Ty::Builtin(BuiltinType::I32),
                            vec![Value::I32(1)],
                        )
                        .unwrap();
                    *read_previous.lock().unwrap() = Some(value);
                    Ok(Value::Array(value))
                })
                .with_prepare_write(move |cx, _, record| {
                    let value = record.new_value;
                    let runtime = cx.runtime();
                    // The scoped host descriptor and both script arrays are roots.
                    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 3);
                    assert_eq!(
                        runtime
                            .gc()
                            .array_get(write_previous.lock().unwrap().unwrap(), 0),
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
    let next = runtime
        .alloc_array(
            &allocation,
            Ty::Builtin(BuiltinType::I32),
            vec![Value::I32(2)],
        )
        .unwrap();
    runtime
        .set_host_path(
            &runtime.gc().alloc_host_root(root).unwrap(),
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
        .alloc_array(&foreign_owner, Ty::Builtin(BuiltinType::I32), vec![])
        .unwrap();
    assert!(
        runtime
            .set_host_path(
                &runtime.gc().alloc_host_root(root).unwrap(),
                descriptor,
                vec![],
                Value::Array(other)
            )
            .is_err()
    );
    assert!(runtime.host_dirty_paths().is_empty());
    assert_eq!(runtime.collect_garbage().unwrap().live_objects, 0);
}

#[test]
fn rejects_disconnected_path_types_before_publishing_descriptors() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let player = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let context = DefinitionContext::new().unwrap();
    let player_declaration = nominal(&context, "game::Player");
    let other_declaration = nominal(&context, "game::Other");
    let field = |owner_type| {
        HostPathSegmentDeclaration::Field(
            context
                .intern_child(
                    if owner_type == player {
                        player_declaration
                    } else {
                        other_declaration
                    },
                    DefinitionKind::Field,
                    "hp",
                    0,
                )
                .unwrap(),
        )
    };
    let index = |collection_type| {
        HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
            slot: 0,
            collection: if collection_type == player {
                Ty::Host(player_declaration)
            } else {
                Ty::Builtin(BuiltinType::I32)
            },
            index: Ty::Builtin(BuiltinType::I32),
            result: Ty::Builtin(BuiltinType::I32),
            access: PathAccess::ReadWrite,
        })
    };
    for segments in [
        vec![field(scalar)],
        vec![index(scalar)],
        vec![field(player), field(player)],
        vec![field(player), index(player)],
    ] {
        let result = runtime.register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: context.snapshot(),
            root_type: player,
            result_type: scalar,
            segments,
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
    let fresh_player = register_host_root_type(&mut fresh, "game::Player", PathAccess::ReadWrite);
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
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();

    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentDeclaration::Field(host_field(
                &runtime, player_id, "hp",
            ))],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
    assert!(
        !runtime
            .gc()
            .alloc_host_root(root)
            .unwrap()
            .is_storable(runtime.gc())
    );
    assert!(
        !runtime
            .gc()
            .alloc_host_path(view)
            .unwrap()
            .is_storable(runtime.gc())
    );
}

#[test]
fn validates_dynamic_index_argument_shape_for_path_views() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let item_id = register_host_root_type(&mut runtime, "game::Item", PathAccess::ReadWrite);
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: player_id,
            result_type: i32_id,
            segments: vec![
                HostPathSegmentDeclaration::Index(HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: Ty::Host(host_declaration(&runtime, player_id)),
                    index: Ty::Builtin(BuiltinType::I32),
                    result: Ty::Host(host_declaration(&runtime, item_id)),
                    access: PathAccess::ReadWrite,
                }),
                HostPathSegmentDeclaration::Field(host_field(&runtime, item_id, "count")),
            ],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
    let borrow = runtime
        .gc()
        .alloc_host_ref(
            frame
                .borrow_shared(HostObjectId(99), i32_id)
                .expect("borrow should be created"),
        )
        .unwrap();
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
fn installed_host_paths_are_available() {
    let calls = Arc::new(Mutex::new(0usize));
    let calls_for_read = Arc::clone(&calls);
    let mut runtime = Runtime::default();
    let owner = root_module(
        "game::Player",
        PathAccess::ReadWrite,
        vec![("hp", Ty::Builtin(BuiltinType::I32))],
    );
    let declaration = owner.declaration();
    let player = declaration
        .definition(DefinitionKind::Struct, "Player", owner.definitions())
        .unwrap();
    let hp = owner
        .definitions()
        .lookup_child(player, DefinitionKind::Field, "hp", 0)
        .unwrap();
    let mut builder = ModuleBuilder::from_declaration(
        owner.to_declaration(),
        &Default::default(),
        BTreeMap::new(),
    )
    .unwrap();
    let path = builder
        .define_path(
            DefinitionMetadata::checked(
                owner.definitions().clone(),
                HostPathDeclaration {
                    root: player,
                    segments: vec![HostPathSegmentDeclaration::Field(hp)],
                    access: PathAccess::ReadOnly,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    builder
        .bind_path(
            &path,
            HostPathAdapter::new().with_read(move |_, _| {
                *calls_for_read.lock().expect("read counter should lock") += 1;
                Ok(Value::I32(7))
            }),
        )
        .unwrap();
    builder.finish().unwrap().install(&mut runtime).unwrap();
    let player_id = runtime.types().get_by_name("game::Player").unwrap().id;
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime.resolve_host_path(&path).unwrap();

    assert_eq!(
        runtime
            .read_host_path(
                &runtime.gc().alloc_host_root(root).unwrap(),
                descriptor_id,
                Vec::new()
            )
            .unwrap(),
        Value::I32(7)
    );
    assert_eq!(*calls.lock().expect("read counter should lock"), 1);
}

#[test]
fn path_execution_validates_stale_roots_and_dynamic_indexes() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentDeclaration::Index(
                HostIndexSegmentDeclaration {
                    slot: 0,
                    collection: Ty::Host(host_declaration(&runtime, player_id)),
                    index: Ty::Builtin(BuiltinType::I32),
                    result: Ty::Builtin(BuiltinType::I32),
                    access: PathAccess::ReadOnly,
                },
            )],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
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
            .read_host_path(
                &runtime.gc().alloc_host_root(root).unwrap(),
                descriptor_id,
                vec![Value::I32(4)]
            )
            .unwrap(),
        Value::I32(40)
    );
    assert_eq!(
        runtime
            .read_host_path(
                &runtime.gc().alloc_host_root(root).unwrap(),
                descriptor_id,
                Vec::new()
            )
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );

    let stale = runtime
        .register_host_root(HostObjectId(2), root.type_id(), HostSchemaEpoch::new(1))
        .unwrap();
    assert_eq!(
        runtime
            .read_host_path(
                &runtime.gc().alloc_host_root(stale).unwrap(),
                descriptor_id,
                vec![Value::I32(1)]
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
    let read_only_id =
        register_host_root_type(&mut runtime, "game::ReadOnly", PathAccess::ReadOnly);
    let opaque_id = register_host_root_type(&mut runtime, "game::Opaque", PathAccess::None);

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
                definitions: runtime.host().definitions(),
                root_type: read_only_id,
                result_type: i32_id,
                segments: vec![HostPathSegmentDeclaration::Field(host_field(
                    &runtime,
                    read_only_id,
                    "hp"
                ))],
                access: PathAccess::ReadWrite,
                schema_epoch: HostSchemaEpoch::new(0),
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
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: runtime.host().definitions(),
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegmentDeclaration::Field(host_field(
                &runtime, player_id, "hp",
            ))],
            access: PathAccess::ReadWrite,
            schema_epoch: HostSchemaEpoch::new(0),
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
