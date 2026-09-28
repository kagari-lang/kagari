use super::*;
use kagari_abi::budget::LogicalBudgetCharge;

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
    use kagari_bytecode::{BytecodeProgram, ModuleRef};
    use kagari_common::cancellation::CancellationToken;
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
                                "execute" => runtime
                                    .consume_logical_charge(LogicalBudgetCharge::Step)
                                    .unwrap_err(),
                                "allocate" => runtime.alloc_array(vec![]).unwrap_err(),
                                "collect" => runtime.collect_garbage().unwrap_err(),
                                "root" => {
                                    assert!(runtime.root_value(Value::Array(array)).is_none());
                                    runtime.resources().ensure_execution_allowed().unwrap_err()
                                }
                                "mutate" => {
                                    assert!(
                                        runtime.gc().array_set(array, 0, Value::I32(2)).is_err()
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
            segments: vec![HostPathSegmentRegistration::Field {
                declaration: runtime
                    .host()
                    .host_type(player_id)
                    .unwrap()
                    .declaration
                    .fields
                    .iter()
                    .find(|field| field.name == "secure_hp")
                    .unwrap()
                    .id
                    .clone(),
            }],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
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
