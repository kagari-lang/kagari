use kagari_runtime::host::PreparedHostPathWrite;
use std::sync::{Arc, Mutex};

use kagari_ir::bytecode::BinaryOp;
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, DynamicPathArgSlot, DynamicPathArgument, DynamicPathArguments,
    FieldMetadataId, HostBorrowTable, HostExposurePolicy, HostObjectId, HostPathAdapter,
    HostPathDescriptorId, HostPathDescriptorRegistration, HostPathOperation, HostPathSegment,
    HostReflectionPolicy, HostSchemaEpoch, HostTypeOwnership, HostTypeRegistration,
    LanguageProfile, PathAccess, Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, TypeId,
    TypeKind, TypeRegistration, host::HostError, value::Value,
};

fn path_mutation_runtime() -> Runtime {
    Runtime::new(path_mutation_config())
}

fn path_mutation_config() -> RuntimeConfig {
    RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_path_mutation: true,
                ..LanguageProfile::default()
            },
            capabilities: CapabilitySet {
                path_mutation: true,
                ..CapabilitySet::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_types: vec![
                "game.Player".to_owned(),
                "game.Item".to_owned(),
                "game.ReadOnly".to_owned(),
                "game.Opaque".to_owned(),
            ],
            allow_host_path_reads: true,
            allow_host_path_mutation: true,
            ..HostExposurePolicy::default()
        },
        ..RuntimeConfig::default()
    }
}

#[test]
fn host_borrows_and_path_operations_share_conflicts_and_release_before_retry() {
    use std::{cell::Cell, rc::Rc};
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let calls = Rc::new(Cell::new(0));
    let read_calls = calls.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| {
                    read_calls.set(read_calls.get() + 1);
                    Ok(Value::I32(10))
                })
                .with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {}))),
        )
        .unwrap();
    let scope = runtime.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_unique(root.object_id(), root.type_id())
        .unwrap();
    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor, vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert_eq!(calls.get(), 0);
    drop(scope);
    let scope = runtime.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_shared(root.object_id(), root.type_id())
        .unwrap();
    runtime
        .read_host_path(&Value::HostRoot(root), descriptor, vec![])
        .unwrap();
    assert_eq!(
        runtime
            .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert!(runtime.host_dirty_paths().is_empty());
    drop(scope);
    runtime
        .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
        .unwrap();
    assert_eq!(runtime.host_dirty_paths().len(), 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn path_callback_borrows_cannot_escape_in_read_results() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new().with_read(move |call, _| {
                let token = call
                    .borrows()
                    .borrow_shared(HostObjectId(2), owner)
                    .unwrap();
                Ok(Value::host_ref(token))
            }),
        )
        .unwrap();
    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor, vec![])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowEscape
    );
    let scope = runtime.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(1), owner)
        .unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(2), owner)
        .unwrap();
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn read_validation_and_preparation_failures_leave_the_target_and_ledger_unchanged() {
    use std::{cell::Cell, rc::Rc};
    for stage in ["read", "validation", "preparation"] {
        let mut runtime = path_mutation_runtime();
        let scalar = register_i32(&runtime);
        let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
        let root = runtime
            .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
        let target = Rc::new(Cell::new(10));
        let read_target = target.clone();
        let prepare_target = target.clone();
        let reject = Rc::new(Cell::new(true));
        let read_reject = reject.clone();
        let validate_reject = reject.clone();
        let prepare_reject = reject.clone();
        runtime
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_validate(move |_, _, _, _| {
                        if stage == "validation" && validate_reject.get() {
                            return Err(HostError::new("rejected validation"));
                        }
                        Ok(())
                    })
                    .with_read(move |_, _| {
                        if stage == "read" && read_reject.get() {
                            return Err(HostError::new("rejected read"));
                        }
                        Ok(Value::I32(read_target.get()))
                    })
                    .with_prepare_write(move |_, _, record| {
                        if stage == "preparation" && prepare_reject.get() {
                            return Err(HostError::new("rejected preparation"));
                        }
                        let Value::I32(next) = record.new_value else {
                            return Err(HostError::new("expected i32"));
                        };
                        let target = prepare_target.clone();
                        Ok(PreparedHostPathWrite::new(move || target.set(next)))
                    }),
            )
            .unwrap();
        for modifying in [false, true] {
            let error = if modifying {
                runtime
                    .modify_host_path(
                        &Value::HostRoot(root),
                        descriptor,
                        vec![],
                        BinaryOp::Add,
                        Value::I32(2),
                    )
                    .unwrap_err()
            } else {
                runtime
                    .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
                    .unwrap_err()
            };
            assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
            assert!(error.message().contains(stage));
            assert_eq!(target.get(), 10);
            assert!(runtime.host_dirty_paths().is_empty());
            assert_eq!(runtime.gc().active_roots(), 0);
            assert!(!runtime.is_quarantined());
        }
        reject.set(false);
        runtime
            .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
            .unwrap();
        assert_eq!(target.get(), 20);
        assert_eq!(runtime.host_dirty_paths().len(), 1);
    }
}

#[test]
fn cancellation_during_a_prepared_commit_is_observed_after_the_atomic_update() {
    use kagari_common::cancellation::CancellationToken;
    use kagari_ir::bytecode::{BytecodeProgram, ModuleRef};
    use std::{cell::Cell, rc::Rc};
    let mut runtime = path_mutation_runtime();
    let module = runtime
        .load_program(
            "commit.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![Default::default()],
            },
        )
        .unwrap();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let target = Rc::new(Cell::new(10));
    let read_target = target.clone();
    let write_target = target.clone();
    let token = CancellationToken::default();
    let cancel = token.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(Value::I32(read_target.get())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(next) = record.new_value else {
                        return Err(HostError::new("expected i32"));
                    };
                    let target = write_target.clone();
                    let cancel = cancel.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        target.set(next);
                        cancel.cancel();
                    }))
                }),
        )
        .unwrap();
    let mut options = runtime.execution_options();
    options.cancellation = token;
    let session = runtime.begin_execution(&module, options).unwrap();
    runtime
        .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
        .unwrap();
    assert_eq!(target.get(), 20);
    assert_eq!(runtime.host_dirty_paths().len(), 1);
    assert_eq!(session.host_scope_count(), 0);
    assert_eq!(
        runtime.gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    drop(session);
    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor, vec![])
            .unwrap(),
        Value::I32(20)
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn nonstorable_previous_value_cannot_escape_through_the_dirty_ledger() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(Value::HostRoot(root)))
                .with_prepare_write(|_, _, _| {
                    panic!("invalid dirty payload must reject before host preparation")
                }),
        )
        .unwrap();
    let error = runtime
        .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
    assert!(runtime.host_dirty_paths().is_empty());
    assert_eq!(runtime.gc().active_roots(), 0);
    assert!(!runtime.is_quarantined());
}

#[test]
fn dirty_record_limit_discards_prepared_resources_without_committing_target() {
    use std::{cell::Cell, rc::Rc};
    struct Reservation(Rc<Cell<usize>>);
    impl Drop for Reservation {
        fn drop(&mut self) {
            self.0.set(self.0.get() - 1);
        }
    }
    let mut config = path_mutation_config();
    config.resources.max_dirty_records = Some(1);
    let mut runtime = Runtime::new(config);
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let target = Rc::new(Cell::new(10));
    let read_target = target.clone();
    let write_target = target.clone();
    let reservations = Rc::new(Cell::new(0));
    let prepare_reservations = reservations.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(Value::I32(read_target.get())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(next) = record.new_value else {
                        return Err(HostError::new("expected i32"));
                    };
                    prepare_reservations.set(prepare_reservations.get() + 1);
                    let reservation = Reservation(prepare_reservations.clone());
                    let target = write_target.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        target.set(next);
                        drop(reservation);
                    }))
                }),
        )
        .unwrap();
    runtime
        .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
        .unwrap();
    let before = runtime.host_dirty_paths();
    for modifying in [false, true] {
        let error = if modifying {
            runtime
                .modify_host_path(
                    &Value::HostRoot(root),
                    descriptor,
                    vec![],
                    BinaryOp::Add,
                    Value::I32(2),
                )
                .unwrap_err()
        } else {
            runtime
                .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(30))
                .unwrap_err()
        };
        assert_eq!(error.kind(), RuntimeErrorKind::ResourceLimitExceeded);
        assert!(error.message().contains("dirty records"));
        assert_eq!(target.get(), 20);
        assert_eq!(runtime.host_dirty_paths(), before);
        assert_eq!(reservations.get(), 0);
        assert_eq!(runtime.gc().active_roots(), 0);
        assert!(!runtime.is_quarantined());
    }
    runtime.clear_host_dirty_paths();
    runtime
        .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(40))
        .unwrap();
    assert_eq!(target.get(), 40);
    assert_eq!(
        runtime.host_dirty_paths()[0].old_value,
        Some(Value::I32(20))
    );
}

#[test]
fn commit_panics_and_execution_attempts_quarantine_only_the_affected_runtime() {
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    for fault in ["panic", "execute", "allocate", "mutate", "collect", "root"] {
        let mut runtime = path_mutation_runtime();
        let scalar = register_i32(&runtime);
        let owner = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
        let root = runtime
            .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
        let array = runtime.alloc_array(vec![Value::I32(1)]).unwrap();
        let access = Rc::new(RefCell::new(None::<Weak<Runtime>>));
        let prepare_access = access.clone();
        let committed = Rc::new(Cell::new(false));
        let prepare_committed = committed.clone();
        runtime
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_read(|_, _| Ok(Value::I32(10)))
                    .with_prepare_write(move |_, _, _| {
                        let access = prepare_access.clone();
                        let committed = prepare_committed.clone();
                        Ok(PreparedHostPathWrite::new(move || {
                            committed.set(true);
                            let runtime = access.borrow().as_ref().unwrap().upgrade().unwrap();
                            let error = match fault {
                                "panic" => panic!("broken host commit invariant"),
                                "execute" => runtime.consume_instruction_step().unwrap_err(),
                                "allocate" => runtime.alloc_array(vec![]).unwrap_err(),
                                "collect" => runtime.collect_garbage().unwrap_err(),
                                "root" => {
                                    assert!(runtime.root_value(Value::Array(array)).is_none());
                                    runtime.resources().ensure_execution_allowed().unwrap_err()
                                }
                                "mutate" => {
                                    assert!(
                                        runtime.gc().array_set(array, 0, Value::I32(2)).is_none()
                                    );
                                    runtime.resources().ensure_execution_allowed().unwrap_err()
                                }
                                _ => unreachable!(),
                            };
                            assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
                            // Swallowing the rejected nested operation cannot restore the runtime.
                        }))
                    }),
            )
            .unwrap();
        let runtime = Rc::new(runtime);
        *access.borrow_mut() = Some(Rc::downgrade(&runtime));
        let error = runtime
            .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(20))
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
        assert!(committed.get());
        assert!(runtime.is_quarantined());
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(runtime.gc().array_get(array, 0), Some(Value::I32(1)));
        assert_eq!(runtime.resources().counters().instruction_steps, 0);
        assert_eq!(runtime.resources().counters().allocation_units, 2);
        assert_eq!(
            runtime.alloc_array(vec![]).unwrap_err().kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(
            runtime
                .read_host_path(&Value::HostRoot(root), descriptor, vec![])
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert_eq!(
            runtime
                .set_host_path(&Value::HostRoot(root), descriptor, vec![], Value::I32(30))
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::EngineFault
        );
        assert!(Runtime::default().alloc_array(vec![]).is_ok());
    }
}

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

fn register_hp_descriptor(
    runtime: &mut Runtime,
    player_id: TypeId,
    i32_id: TypeId,
    access: PathAccess,
) -> HostPathDescriptorId {
    runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            root_type: player_id,
            result_type: i32_id,
            segments: vec![HostPathSegment::Field {
                name: "hp".to_owned(),
                field_id: FieldMetadataId::new(0),
                owner_type: player_id,
                result_type: i32_id,
                access,
                abi_fingerprint: AbiFingerprint(21),
            }],
            access,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(22),
            capability_requirements: CapabilitySet::default(),
        })
        .unwrap()
}

#[test]
fn heap_path_temporaries_survive_collection_during_write_preparation() {
    use std::{
        cell::{Cell, RefCell},
        rc::{Rc, Weak},
    };
    let mut runtime = path_mutation_runtime();
    let result_type = runtime
        .types()
        .register(TypeRegistration::new("[i32]", TypeKind::Array))
        .unwrap();
    let player_type = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_type, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(
        &mut runtime,
        player_type,
        result_type,
        PathAccess::ReadWrite,
    );
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
                    let value = runtime.alloc_array(vec![Value::I32(1)]).unwrap();
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
    let next = runtime.alloc_array(vec![Value::I32(2)]).unwrap();
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
    let foreign = Runtime::default();
    let other = foreign.alloc_array(vec![]).unwrap();
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
    let mut runtime = path_mutation_runtime();
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
            segments: vec![HostPathSegment::Index {
                slot: DynamicPathArgSlot::new(0),
                collection_type: player_id,
                index_type: i32_id,
                result_type: i32_id,
                access: PathAccess::ReadOnly,
                abi_fingerprint: AbiFingerprint(71),
            }],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(72),
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

#[test]
fn executes_path_read_prepare_commit_and_dirty_records_in_order() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor_id =
        register_hp_descriptor(&mut runtime, player_id, i32_id, PathAccess::ReadWrite);
    let hp = Arc::new(Mutex::new(10));
    let events = Arc::new(Mutex::new(Vec::<String>::new()));
    let read_hp = Arc::clone(&hp);
    let write_hp = Arc::clone(&hp);
    let validate_events = Arc::clone(&events);
    let dirty_events = Arc::clone(&events);

    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new()
                .with_validate(move |_, _, operation, value| {
                    validate_events
                        .lock()
                        .unwrap()
                        .push(format!("validate:{operation:?}:{}", value.is_some()));
                    Ok(())
                })
                .with_read(move |_, _| Ok(Value::I32(*read_hp.lock().unwrap())))
                .with_prepare_write(move |_, _, record| {
                    let Value::I32(value) = record.new_value else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    let event = format!(
                        "dirty:{:?}:{:?}->{:?}",
                        record.operation, record.old_value, record.new_value
                    );
                    dirty_events
                        .lock()
                        .unwrap()
                        .try_reserve(1)
                        .map_err(|_| HostError::new("event capacity"))?;
                    let write_hp = write_hp.clone();
                    let dirty_events = dirty_events.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        *write_hp.lock().unwrap() = value;
                        dirty_events.lock().unwrap().push(event);
                    }))
                }),
        )
        .unwrap();
    let root_value = Value::HostRoot(root);

    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor_id, Vec::new())
            .unwrap(),
        Value::I32(10)
    );
    runtime
        .set_host_path(&root_value, descriptor_id, Vec::new(), Value::I32(20))
        .unwrap();
    assert_eq!(*hp.lock().unwrap(), 20);
    assert_eq!(
        runtime
            .modify_host_path(
                &root_value,
                descriptor_id,
                Vec::new(),
                BinaryOp::Sub,
                Value::I32(3),
            )
            .unwrap(),
        Value::I32(17)
    );
    assert_eq!(*hp.lock().unwrap(), 17);

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "validate:Read:false".to_owned(),
            "validate:Set:true".to_owned(),
            "dirty:Set:Some(I32(10))->I32(20)".to_owned(),
            "validate:Modify(Sub):true".to_owned(),
            "dirty:Modify(Sub):Some(I32(20))->I32(17)".to_owned(),
        ]
    );
    let dirty = runtime.host_dirty_paths();
    assert_eq!(dirty.len(), 2);
    assert_eq!(dirty[0].operation, HostPathOperation::Set);
    assert_eq!(dirty[1].operation, HostPathOperation::Modify(BinaryOp::Sub));
}

#[test]
fn path_execution_classifies_validation_failures() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let read_only = register_hp_descriptor(&mut runtime, player_id, i32_id, PathAccess::ReadOnly);
    runtime
        .register_host_path_adapter(
            read_only,
            HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(1))),
        )
        .unwrap();
    let root_value = Value::HostRoot(root);

    assert_eq!(
        runtime
            .set_host_path(&root_value, read_only, Vec::new(), Value::I32(2))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime
            .read_host_path(&Value::I32(1), read_only, Vec::new())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
}

#[test]
fn arithmetic_path_failure_never_calls_write_or_records_dirty() {
    for (initial, op, rhs, message) in [
        (i32::MAX, BinaryOp::Add, 1, "integer overflow"),
        (i32::MIN, BinaryOp::Sub, 1, "integer overflow"),
        (50_000, BinaryOp::Mul, 50_000, "integer overflow"),
        (i32::MIN, BinaryOp::Div, -1, "integer overflow"),
        (7, BinaryOp::Div, 0, "integer division by zero"),
    ] {
        let mut runtime = path_mutation_runtime();
        let i32_id = register_i32(&runtime);
        let player_id = register_host_root_type(&mut runtime, "game.Player", PathAccess::ReadWrite);
        let root = runtime
            .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor =
            register_hp_descriptor(&mut runtime, player_id, i32_id, PathAccess::ReadWrite);
        let value = Arc::new(Mutex::new(initial));
        let read_value = value.clone();
        let write_value = value.clone();
        let writes = Arc::new(Mutex::new(0));
        let write_count = writes.clone();
        runtime
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_read(move |_, _| Ok(Value::I32(*read_value.lock().unwrap())))
                    .with_prepare_write(move |_, _, record| {
                        let Value::I32(value) = record.new_value else {
                            return Err(HostError::new("expected i32"));
                        };
                        let write_count = write_count.clone();
                        let write_value = write_value.clone();
                        Ok(PreparedHostPathWrite::new(move || {
                            *write_count.lock().unwrap() += 1;
                            *write_value.lock().unwrap() = value;
                        }))
                    }),
            )
            .unwrap();
        let error = runtime
            .modify_host_path(
                &Value::HostRoot(root),
                descriptor,
                vec![],
                op,
                Value::I32(rhs),
            )
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert_eq!(error.message(), message);
        assert_eq!(*value.lock().unwrap(), initial);
        assert_eq!(*writes.lock().unwrap(), 0);
        assert!(runtime.host_dirty_paths().is_empty());
    }
}

#[test]
fn path_execution_enforces_descriptor_capabilities() {
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
            segments: vec![HostPathSegment::Field {
                name: "secure_hp".to_owned(),
                field_id: FieldMetadataId::new(2),
                owner_type: player_id,
                result_type: i32_id,
                access: PathAccess::ReadOnly,
                abi_fingerprint: AbiFingerprint(61),
            }],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
            abi_fingerprint: AbiFingerprint(62),
            capability_requirements: CapabilitySet {
                reflection_read: true,
                ..CapabilitySet::default()
            },
        })
        .unwrap();
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new().with_read(|_, _| Ok(Value::I32(99))),
        )
        .unwrap();

    assert_eq!(
        runtime
            .read_host_path(&Value::HostRoot(root), descriptor_id, Vec::new())
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::CapabilityDenied
    );
}
