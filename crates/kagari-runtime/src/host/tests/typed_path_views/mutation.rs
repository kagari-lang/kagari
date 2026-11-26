use super::*;
use crate::host::HostCallContext;
#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::test_support::native::program::ProgramFixture;
use crate::value::{arrays::ArrayObject, carriers::RootedValue, view::ValueView};
use kagari_abi::representation::ValueType;
use kagari_common::cancellation::CancellationToken;
use kagari_types::declaration::path::HostPathDeclaration;
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
    assert_eq!(
        scope
            .runtime_mut()
            .retire_host_root(root)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
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
    assert_eq!(
        scope
            .runtime_mut()
            .retire_host_root(root)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    drop(scope);
    runtime
        .set_host_path(&root_value, descriptor, &[], &RootedValue::from(20))
        .unwrap();
    assert_eq!(runtime.host_dirty_paths().len(), 1);
    assert_eq!(runtime.gc_stats().root_registrations, baseline_roots);
}

#[test]
fn retired_root_and_retained_view_never_resolve_to_a_same_schema_replacement() {
    let mut runtime = path_mutation_runtime();
    let scalar = register_i32(&runtime);
    let player = register_host_root_type(&mut runtime, "game::Player", PathAccess::ReadOnly);
    let epoch = HostSchemaEpoch::new(0);
    let a = runtime
        .register_host_root(HostObjectId(7), player, epoch)
        .unwrap();
    let hp = register_hp_descriptor(&mut runtime, player, scalar, PathAccess::ReadOnly);
    let reads = Arc::new(Mutex::new(Vec::new()));
    let observed = reads.clone();
    runtime
        .register_host_path_adapter(
            hp,
            HostPathAdapter::new().with_read(move |_, path| {
                observed.lock().unwrap().push(path.root);
                Ok(ValueView::from(10))
            }),
        )
        .unwrap();
    let value = runtime.host_root_value(a).unwrap();
    let view = runtime.make_host_path_view(&value, hp, &[]).unwrap();
    let mut call = HostCallContext::new(&mut runtime, &[]).unwrap();
    let view = call.import(&view).unwrap();
    assert_eq!(call.inspect_path(&view).unwrap().root, a);
    call.runtime_mut().retire_host_root(a).unwrap();
    assert_eq!(call.runtime().host().root(a.object_id()), None);
    let b = call
        .runtime_mut()
        .register_host_root(HostObjectId(8), player, epoch)
        .unwrap();
    assert_eq!(
        call.inspect_path(&view).unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    drop(call);
    assert_eq!(runtime.host().roots().collect::<Vec<_>>(), [b]);
    assert_eq!(
        runtime
            .register_host_root(a.object_id(), player, epoch)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::MetadataConflict
    );
    assert_eq!(
        runtime.retire_host_root(a).unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime.host_root_value(a).unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    assert_eq!(
        runtime.read_host_path(&value, hp, &[]).unwrap_err().kind(),
        RuntimeErrorKind::TypedPathValidation
    );
    let fresh = runtime.host_root_value(b).unwrap();
    assert_eq!(
        runtime.read_host_path(&fresh, hp, &[]).unwrap().as_i32(),
        Some(10)
    );
    assert_eq!(*reads.lock().unwrap(), [b]);
}

#[test]
fn path_callback_releases_its_own_and_automatic_borrows_after_failure() {
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
                call.borrows()
                    .borrow_shared(HostObjectId(2), owner)
                    .unwrap();
                Err(HostError::new("read failed after reservation"))
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
        RuntimeErrorKind::TypedPathValidation
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
fn bound_whole_path_mutates_below_val_but_a_separate_view_cannot() {
    struct Stats {
        hp: i32,
    }

    struct Player {
        baseline: Stats,
    }

    let context = DefinitionContext::new().unwrap();
    let player = nominal(&context, "game::Player");
    let stats = nominal(&context, "game::Stats");
    let field = |owner, name| {
        HostPathSegmentDeclaration::Field(
            context
                .intern_child(owner, DefinitionKind::Field, name, 0)
                .unwrap(),
        )
    };
    let baseline = field(player, "baseline");
    let hp = field(stats, "hp");
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    for (owner, name, ty, mutable, access) in [
        (
            "Stats",
            "hp",
            Ty::Builtin(BuiltinType::I32),
            true,
            PathAccess::ReadWrite,
        ),
        (
            "Player",
            "baseline",
            Ty::Host(stats),
            false,
            PathAccess::ReadOnly,
        ),
    ] {
        let mut owner = module.define_type(owner).unwrap();
        owner
            .external(HostTypePolicy {
                ownership: HostTypeOwnership::HostRoot,
                reflection: HostReflectionPolicy::Metadata,
                path_access: PathAccess::ReadWrite,
            })
            .unwrap();
        owner
            .field(
                DefinitionMetadata::checked(
                    context.snapshot(),
                    FieldDef {
                        name: name.into(),
                        ty,
                        mutable,
                        visibility: Visibility::Public,
                        host_policy: Some(HostFieldPolicy {
                            readable: true,
                            path_access: access,
                        }),
                    },
                    &Default::default(),
                )
                .unwrap(),
            )
            .unwrap();
        owner.finish().unwrap();
    }
    let path = module
        .define_path(
            DefinitionMetadata::checked(
                context.snapshot(),
                HostPathDeclaration {
                    root: player,
                    segments: vec![baseline.clone(), hp],
                    access: PathAccess::ReadWrite,
                    schema_epoch: 0,
                },
                &Default::default(),
            )
            .unwrap(),
        )
        .unwrap();
    let state = Arc::new(Mutex::new(Player {
        baseline: Stats { hp: 10 },
    }));
    let read = state.clone();
    let adapter = HostPathAdapter::new()
        .with_read(move |_, _| Ok(ValueView::from(read.lock().unwrap().baseline.hp)));
    assert_eq!(
        module.bind_path(&path, adapter.clone()).unwrap_err().kind(),
        RuntimeErrorKind::MetadataConflict
    );
    let write = state.clone();
    let adapter = adapter.with_prepare_write(move |_, _, record| {
        let value = record.new_value.as_i32().unwrap();
        let write = write.clone();
        Ok(PreparedHostPathWrite::new(move || {
            write.lock().unwrap().baseline.hp = value;
        }))
    });
    module.bind_path(&path, adapter.clone()).unwrap();
    let mut runtime = path_mutation_runtime();
    module.finish().unwrap().install(&mut runtime).unwrap();
    let id = runtime.resolve_host_path(&path).unwrap();
    let descriptor = runtime.host().path_descriptor(id).unwrap();
    assert_eq!(descriptor.segments[0].access(), PathAccess::ReadOnly);
    assert_eq!(descriptor.segments[1].access(), PathAccess::ReadWrite);
    let (player, scalar) = (descriptor.root_type, descriptor.result_type);
    let stats = descriptor.segments[0].result_type();
    let root = runtime
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let root = runtime.host_root_value(root).unwrap();
    runtime
        .set_host_path(&root, id, &[], &RootedValue::from(20))
        .unwrap();
    assert_eq!(state.lock().unwrap().baseline.hp, 20);
    let prefix = runtime
        .register_host_path_descriptor(HostPathDescriptorRegistration {
            definitions: context.snapshot(),
            root_type: player,
            result_type: stats,
            segments: vec![baseline],
            access: PathAccess::ReadOnly,
            schema_epoch: HostSchemaEpoch::new(0),
        })
        .unwrap();
    let view = runtime.make_host_path_view(&root, prefix, &[]).unwrap();
    let suffix = register_hp_descriptor(&mut runtime, stats, scalar, PathAccess::ReadWrite);
    runtime.register_host_path_adapter(suffix, adapter).unwrap();
    let error = runtime
        .set_host_path(&view, suffix, &[], &RootedValue::from(99))
        .unwrap_err();
    assert_eq!(error.kind(), RuntimeErrorKind::TypedPathValidation);
    assert!(
        error.to_string().contains("host path view is not writable"),
        "{error}"
    );
    assert_eq!(state.lock().unwrap().baseline.hp, 20);
    assert_eq!(runtime.host_dirty_paths().len(), 1);
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
fn narrow_path_add_checks_the_old_value_and_sum_before_preparing_a_write() {
    let mut runtime = path_mutation_runtime();
    root_module(
        "game::Player",
        PathAccess::ReadWrite,
        vec![("hp", Ty::Builtin(BuiltinType::I8))],
    )
    .install(&mut runtime)
    .unwrap();
    let player = runtime.types().get_by_name("game::Player").unwrap().id;
    let leaf = runtime.types().get_by_name("i8").unwrap().id;
    let descriptor = register_hp_descriptor(&mut runtime, player, leaf, PathAccess::ReadWrite);
    let root = runtime
        .register_host_root(HostObjectId(1), player, HostSchemaEpoch::new(0))
        .unwrap();
    let root = runtime.host_root_value(root).unwrap();
    let hp = Arc::new(AtomicI32::new(127));
    let prepares = Arc::new(AtomicUsize::new(0));
    let (read, write, count) = (hp.clone(), hp.clone(), prepares.clone());
    runtime
        .register_host_path_adapter(
            descriptor,
            HostPathAdapter::new()
                .with_read(move |_, _| Ok(ValueView::from(read.load(Ordering::SeqCst))))
                .with_prepare_write(move |_, _, change| {
                    count.fetch_add(1, Ordering::SeqCst);
                    let next = change.new_value.as_i32().unwrap();
                    let write = write.clone();
                    Ok(PreparedHostPathWrite::new(move || {
                        write.store(next, Ordering::SeqCst)
                    }))
                }),
        )
        .unwrap();
    // The second sum would fit i8: validating only the result misses the invalid read.
    for (old, rhs, kind) in [
        (127, 1, RuntimeErrorKind::ScriptTrap),
        (128, -1, RuntimeErrorKind::TypedPathValidation),
    ] {
        hp.store(old, Ordering::SeqCst);
        let error = runtime
            .modify_host_path(
                &root,
                descriptor,
                &[],
                BinaryOp::Add,
                &RootedValue::from(rhs),
            )
            .unwrap_err();
        assert_eq!(error.kind(), kind);
        if kind == RuntimeErrorKind::ScriptTrap {
            assert_eq!(error.to_string(), "integer overflow");
        }
        assert_eq!(prepares.load(Ordering::SeqCst), 0);
        assert_eq!(hp.load(Ordering::SeqCst), old);
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
