use super::*;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::{
    execution::roots::ExecutionRoots,
    value::{
        EphemeralValue, arrays::ArrayObject, carriers::RootedValue, host_values::EphemeralObject,
        view::ValueView,
    },
};
use kagari_abi::representation::ValueType;
use kagari_common::cancellation::CancellationToken;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
};

#[test]
fn host_borrows_and_path_operations_share_conflicts_and_release_before_retry() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let calls = Arc::new(AtomicUsize::new(0));
    let read_calls = calls.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| {
                    read_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ValueView::from(10))
                })
                .with_prepare_write(|_, _, _| Ok(PreparedHostPathWrite::new(|| {}))),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    let mut scope = runtime.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_unique(root.object_id(), root.type_id())
        .unwrap();
    assert_eq!(
        scope
            .runtime_mut()
            .read_host_path(&root_value, descriptor, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(scope);
    let mut scope = runtime.host_scope(&[]).unwrap();
    scope
        .borrows()
        .borrow_shared(root.object_id(), root.type_id())
        .unwrap();
    let _ = scope
        .runtime_mut()
        .read_host_path(&root_value, descriptor, &[])
        .unwrap();
    assert_eq!(
        scope
            .runtime_mut()
            .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    assert!(scope.runtime().host_dirty_paths().is_empty());
    drop(scope);
    runtime
        .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
        .unwrap();
    assert_eq!(runtime.host_dirty_paths().len(), 1);
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
}

#[test]
fn path_callback_borrows_cannot_escape_in_read_results() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
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
                let execution = call.scope.execution_id();
                {
                    let Runtime {
                        memory, resources, ..
                    } = call.runtime_mut();
                    memory.scope(resources, |cx, roots| {
                        ExecutionRoots::with(roots, cx, |slots| {
                            let value = EphemeralObject::alloc(
                                slots.scope(),
                                EphemeralValue::HostRef(token),
                            )?;
                            let location = slots.push_local(execution, value)?;
                            ValueView::protected(value, Some(location))
                        })
                    })
                }
                .map_err(|error| HostError::new(error.to_string()))
            }),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;
    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor, &[])
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
    assert_eq!(
        scope.runtime().gc_stats().root_registrations,
        baseline_roots
    );
}

#[test]
fn read_validation_and_preparation_failures_leave_the_target_and_ledger_unchanged() {
    for stage in ["read", "validation", "preparation"] {
        let mut runtime = path_mutation_runtime();
        let scalar = register_i32(&runtime);
        let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
        let root = runtime
            .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
            .unwrap();
        let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
        let target = Arc::new(AtomicI32::new(10));
        let read_target = target.clone();
        let prepare_target = target.clone();
        let reject = Arc::new(AtomicBool::new(true));
        let read_reject = reject.clone();
        let validate_reject = reject.clone();
        let prepare_reject = reject.clone();
        runtime
            .register_host_path_adapter(
                descriptor,
                HostPathAdapter::new()
                    .with_validate(move |_, _, _, _| {
                        if stage == "validation" && validate_reject.load(Ordering::SeqCst) {
                            return Err(HostError::new("rejected validation"));
                        }
                        Ok(())
                    })
                    .with_read(move |_, _| {
                        if stage == "read" && read_reject.load(Ordering::SeqCst) {
                            return Err(HostError::new("rejected read"));
                        }
                        Ok(ValueView::from(read_target.load(Ordering::SeqCst)))
                    })
                    .with_prepare_write(move |_, _, record| {
                        if stage == "preparation" && prepare_reject.load(Ordering::SeqCst) {
                            return Err(HostError::new("rejected preparation"));
                        }
                        let Some(next) = record.new_value.as_i32() else {
                            return Err(HostError::new("expected i32"));
                        };
                        let target = prepare_target.clone();
                        Ok(PreparedHostPathWrite::new(move || {
                            target.store(next, Ordering::SeqCst)
                        }))
                    }),
            )
            .unwrap();
        let root_value = runtime.host_root_value(root).unwrap();
        let baseline_roots = runtime.gc_stats().root_registrations;
        for modifying in [false, true] {
            let error = if modifying {
                runtime
                    .modify_host_path(
                        &root_value,
                        descriptor,
                        &[],
                        BinaryOp::Add,
                        &RootedValue::from(2),
                    )
                    .unwrap_err()
            } else {
                runtime
                    .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
                    .unwrap_err()
            };
            assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
            assert!(error.to_string().contains(stage));
            assert_eq!(target.load(Ordering::SeqCst), 10);
            assert!(runtime.host_dirty_paths().is_empty());
            assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
            assert!(!runtime.is_quarantined());
        }
        reject.store(false, Ordering::SeqCst);
        runtime
            .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
            .unwrap();
        assert_eq!(target.load(Ordering::SeqCst), 20);
        assert_eq!(runtime.host_dirty_paths().len(), 1);
    }
}

#[test]
fn cancellation_during_a_prepared_commit_is_observed_after_the_atomic_update() {
    let context = DefinitionContext::new().unwrap();

    let mut runtime = path_mutation_runtime();
    let module_program = load_bytecode(
        &mut runtime,
        "commit.kgr",
        ProgramFixture {
            exports: Default::default(),
            root: ModuleRef::new(0),
            modules: vec![Default::default()],
        }
        .metadata(context.snapshot(), &Default::default())
        .unwrap(),
    )
    .unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let target = Arc::new(AtomicI32::new(10));
    let read_target = target.clone();
    let write_target = target.clone();
    let token = CancellationToken::default();
    let cancel = token.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(ValueView::from(read_target.load(Ordering::SeqCst))))
                .with_prepare_write(move |_, _, record| {
                    let Some(next) = record.new_value.as_i32() else {
                        return Err(HostError::new("expected i32"));
                    };
                    let target = write_target.clone();
                    let cancel = cancel.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        target.store(next, Ordering::SeqCst);
                        cancel.cancel();
                    }))
                }),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let mut options = runtime.execution_options();
    options.cancellation = token;
    let mut session = runtime.begin_execution(&module, options).unwrap();
    session
        .runtime_mut()
        .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
        .unwrap();
    assert_eq!(target.load(Ordering::SeqCst), 20);
    assert_eq!(session.runtime().host_dirty_paths().len(), 1);
    assert_eq!(session.host_scope_count(), 0);
    assert_eq!(
        session.runtime_mut().gc_safepoint().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert_eq!(session.runtime().gc_stats().root_registrations, roots);
    drop(session);
    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor, &[])
            .unwrap()
            .as_i32(),
        Some(20)
    );
    assert!(!runtime.is_quarantined());
}

#[test]
fn nonstorable_previous_value_cannot_escape_through_the_dirty_ledger() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |call, _| {
                    let value = call.runtime_mut().host_root_value(root).unwrap();
                    Ok(call.import(&value).unwrap())
                })
                .with_prepare_write(|_, _, _| {
                    panic!("invalid dirty payload must reject before host preparation")
                }),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let objects = runtime.gc_stats().objects;
    let error = runtime
        .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
    assert!(runtime.host_dirty_paths().is_empty());
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    assert_eq!(runtime.gc_stats().objects, objects);
    assert!(!runtime.is_quarantined());
}

#[test]
fn commit_panics_quarantine_only_the_affected_runtime() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let owner = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), owner, HostSchemaEpoch::new(0))
        .unwrap();
    let descriptor = register_hp_descriptor(&mut runtime, owner, scalar, PathAccess::ReadWrite);
    let allocation = allocation_owner(&mut runtime);
    let array = runtime
        .alloc_array(
            &allocation,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(1i32)],
        )
        .unwrap();
    let committed = Arc::new(AtomicBool::new(false));
    let prepare_committed = committed.clone();
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new().with_prepare_write(move |_, _, _| {
                let committed = prepare_committed.clone();
                Ok(PreparedHostPathWrite::new(move || {
                    committed.store(true, Ordering::SeqCst);
                    panic!("broken host commit invariant");
                }))
            }),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();
    let roots = runtime.gc_stats().root_registrations;
    let error = runtime
        .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::EngineFault);
    assert!(committed.load(Ordering::SeqCst));
    assert!(runtime.is_quarantined());
    assert_eq!(runtime.gc_stats().root_registrations, roots);
    // Quarantine forbids executable services. This closed physical read is only
    // the independent oracle for the pre-existing array's completed state.
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = cx.downcast::<ArrayObject>(array.get(cx)?.object().unwrap())?;
            assert_eq!(
                ArrayObject::get(cx, object, 0)?.scalar_payload(),
                Some((ValueType::I32, 1))
            );
            Ok(())
        })
        .unwrap();

    assert_eq!(
        runtime
            .alloc_array(&allocation, Ty::Builtin(BuiltinType::I32), &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor, &[])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert_eq!(
        runtime
            .set_host_path(&root_value, descriptor, &[], &RootedValue::from(30))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    let mut unaffected = Runtime::default();
    let unaffected_owner = allocation_owner(&mut unaffected);
    assert!(
        unaffected
            .alloc_array(&unaffected_owner, Ty::Builtin(BuiltinType::I32), &[])
            .is_ok()
    );
}

#[test]
fn executes_path_read_prepare_commit_and_dirty_records_in_order() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
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
                .with_read(move |_, _| Ok(ValueView::from(*read_hp.lock().unwrap())))
                .with_prepare_write(move |_, _, record| {
                    let Some(value) = record.new_value.as_i32() else {
                        return Err(HostError::new("hp expects i32"));
                    };
                    let event = format!(
                        "dirty:{:?}:{:?}->{:?}",
                        record.operation,
                        record.old_value.and_then(|value| value.as_i32()),
                        value
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
    let root_value = runtime.host_root_value(root).unwrap();
    let baseline_roots = runtime.gc_stats().root_registrations;

    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor_id, &[])
            .unwrap()
            .as_i32(),
        Some(10)
    );
    runtime
        .set_host_path(&root_value, descriptor_id, &[], &RootedValue::from(20))
        .unwrap();
    assert_eq!(*hp.lock().unwrap(), 20);
    assert_eq!(
        runtime
            .modify_host_path(
                &root_value,
                descriptor_id,
                &[],
                BinaryOp::Sub,
                &RootedValue::from(3),
            )
            .unwrap()
            .as_i32(),
        Some(17)
    );
    assert_eq!(*hp.lock().unwrap(), 17);

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "validate:Read:false".to_owned(),
            "validate:Set:true".to_owned(),
            "dirty:Set:Some(10)->20".to_owned(),
            "validate:Modify(Sub):true".to_owned(),
            "dirty:Modify(Sub):Some(20)->17".to_owned(),
        ]
    );
    let dirty = runtime.host_dirty_paths();
    assert_eq!(dirty.len(), 2);
    assert_eq!(dirty[0].operation, HostPathOperation::Set);
    assert_eq!(dirty[1].operation, HostPathOperation::Modify(BinaryOp::Sub));
    assert_eq!(
        dirty[0].old_value.as_ref().and_then(RootedValue::as_i32),
        Some(10)
    );
    assert_eq!(dirty[0].new_value.as_i32(), Some(20));
    assert_eq!(
        dirty[1].old_value.as_ref().and_then(RootedValue::as_i32),
        Some(20)
    );
    assert_eq!(dirty[1].new_value.as_i32(), Some(17));
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
}

#[test]
fn path_execution_classifies_validation_failures() {
    let mut runtime = path_mutation_runtime();
    let i32_id = register_i32(&runtime);
    let player_id = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player_id, HostSchemaEpoch::new(0))
        .unwrap();
    let read_only = register_hp_descriptor(&mut runtime, player_id, i32_id, PathAccess::ReadOnly);
    runtime
        .register_host_path_adapter(
            read_only,
            HostPathAdapter::new().with_read(|_, _| Ok(ValueView::from(1))),
        )
        .unwrap();
    let root_value = runtime.host_root_value(root).unwrap();

    assert_eq!(
        runtime
            .set_host_path(&root_value, read_only, &[], &RootedValue::from(2))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime
            .read_host_path(&RootedValue::from(1), read_only, &[])
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
        let player_id =
            register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadWrite);
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
                    .with_read(move |_, _| Ok(ValueView::from(*read_value.lock().unwrap())))
                    .with_prepare_write(move |_, _, record| {
                        let Some(value) = record.new_value.as_i32() else {
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
        let root_value = runtime.host_root_value(root).unwrap();
        let error = runtime
            .modify_host_path(&root_value, descriptor, &[], op, &RootedValue::from(rhs))
            .unwrap_err();
        assert_eq!(error.kind(), RuntimeErrorKind::ScriptTrap);
        assert_eq!(error.to_string(), message);
        assert_eq!(*value.lock().unwrap(), initial);
        assert_eq!(*writes.lock().unwrap(), 0);
        assert!(runtime.host_dirty_paths().is_empty());
    }
}

#[test]
fn path_execution_uses_installed_descriptor() {
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
                &runtime,
                player_id,
                "secure_hp",
            ))],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap();
    runtime
        .register_host_path_adapter(
            descriptor_id,
            HostPathAdapter::new().with_read(|_, _| Ok(ValueView::from(99))),
        )
        .unwrap();

    let root_value = runtime.host_root_value(root).unwrap();
    assert_eq!(
        runtime
            .read_host_path(&root_value, descriptor_id, &[])
            .unwrap()
            .as_i32(),
        Some(99)
    );
}
