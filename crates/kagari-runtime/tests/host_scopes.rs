use kagari_bytecode::{
    module::BytecodeModule,
    program::{BytecodeProgram, ModuleRef},
};
use kagari_contract::ids::FunctionRef;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{
        HostBorrowKind, HostError, HostFunction, HostObjectId, HostSchemaEpoch,
        HostTypeRegistration,
    },
    metadata::TypeId,
    session::TraceValue,
    value::Value,
};
use kagari_types::{
    host_interface::{
        HostFunctionDeclaration, HostParameter, HostPassingStyle,
        type_declaration::{HostTypeDeclaration, HostTypeOwnership, PathAccess},
        value_type::HostValueType,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::{
    cell::{Cell, RefCell},
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..Default::default()
    })
}

fn root(runtime: &mut Runtime) -> Value {
    let mut registration =
        HostTypeRegistration::new(HostTypeDeclaration::new("game.Object"), "Object");
    registration.declaration.ownership = HostTypeOwnership::HostRoot;
    registration.declaration.path_access = PathAccess::ReadWrite;
    let ty = runtime.register_host_type(registration).unwrap();
    Value::HostRoot(
        runtime
            .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
            .unwrap(),
    )
}

fn declaration(name: &str, passing: HostPassingStyle) -> HostFunctionDeclaration {
    HostFunctionDeclaration::new(
        name,
        vec![HostParameter {
            name: "object".into(),
            ty: HostValueType::opaque("game.Object"),
            passing,
        }],
        HostValueType::Unit,
    )
}

#[test]
fn same_frame_numbers_in_different_runtimes_do_not_authorize_foreign_borrows() {
    let first = runtime();
    let second = runtime();
    let a = first.host_scope(&[]).unwrap();
    let b = second.host_scope(&[]).unwrap();
    let at = a
        .borrows()
        .borrow_shared(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let bt = b
        .borrows()
        .borrow_shared(HostObjectId(1), TypeId::new(0))
        .unwrap();
    assert_eq!(at.frame_id(), bt.frame_id());
    assert_eq!(at.epoch(), bt.epoch());
    assert_ne!(at, bt);
    assert_eq!(
        a.borrows()
            .validate(bt, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    assert_eq!(
        second
            .validate_host_borrow(at, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    assert!(
        first
            .host_scope(&[Value::Tuple(vec![Value::host_ref(bt)])])
            .is_err()
    );
    a.borrows().validate(at, HostBorrowKind::Shared).unwrap();
    drop(a);
    assert_eq!(
        first
            .validate_host_borrow(at, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    assert!(!first.is_quarantined());
}

#[test]
fn unwinding_host_scopes_releases_roots_borrows_and_session_retention() {
    for with_session in [false, true] {
        let mut runtime = runtime();
        let loaded = runtime
            .load_program(
                "unwind",
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                },
            )
            .unwrap();
        let value = Value::Array(
            runtime
                .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
                .unwrap(),
        );
        let token = Cell::new(None);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let session = with_session.then(|| {
                    runtime
                        .begin_execution(&loaded, runtime.execution_options())
                        .unwrap()
                });
                let scope = runtime.host_scope(std::slice::from_ref(&value)).unwrap();
                token.set(Some(
                    scope
                        .borrows()
                        .borrow_unique(HostObjectId(9), TypeId::new(0))
                        .unwrap(),
                ));
                if let Some(session) = &session {
                    assert_eq!(session.host_scope_count(), 1);
                }
                runtime.collect_garbage().unwrap();
                assert!(runtime.gc().validate_value(&value));
                panic!("host unwind");
            }))
            .is_err()
        );
        assert!(runtime.execution_root().is_none());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(runtime.gc().active_roots(), 0);
        assert_eq!(
            runtime
                .modules()
                .retention_counts(loaded.key())
                .active_calls,
            0
        );
        assert_eq!(
            runtime
                .validate_host_borrow(token.get().unwrap(), HostBorrowKind::Unique)
                .unwrap_err()
                .kind(),
            RuntimeErrorKind::ExpiredHostBorrow
        );
        let next = runtime.host_scope(&[]).unwrap();
        next.borrows()
            .borrow_unique(HostObjectId(9), TypeId::new(0))
            .unwrap();
        drop(next);
        runtime.collect_garbage().unwrap();
        assert!(!runtime.gc().validate_value(&value));
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn host_scopes_keep_root_cancellation_until_all_resources_are_released() {
    let mut runtime = runtime();
    let loaded = runtime
        .load_program(
            "scope",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let options = runtime.execution_options();

    let cancel = options.cancellation.clone();
    let session = runtime.begin_execution(&loaded, options).unwrap();
    let a = Value::Array(
        runtime
            .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(1)])
            .unwrap(),
    );
    let b = Value::Array(
        runtime
            .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(2)])
            .unwrap(),
    );
    let outer = runtime.host_scope(std::slice::from_ref(&a)).unwrap();
    let inner = runtime.host_scope(std::slice::from_ref(&b)).unwrap();
    let token = outer
        .borrows()
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    assert_eq!(session.host_scope_count(), 2);
    runtime.collect_garbage().unwrap();
    assert!(runtime.gc().validate_value(&a));
    assert!(runtime.gc().validate_value(&b));
    drop(session);
    runtime.resources().poll_execution().unwrap();
    cancel.cancel();
    assert_eq!(
        runtime.resources().poll_execution().unwrap_err().kind(),
        RuntimeErrorKind::Cancelled
    );
    assert!(runtime.host_scope(&[]).is_err());
    assert!(
        outer
            .borrows()
            .borrow_shared(HostObjectId(2), TypeId::new(0))
            .is_err()
    );
    drop(inner);
    assert!(runtime.execution_root().is_some());
    drop(outer);
    assert!(runtime.execution_root().is_none());
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(
        runtime
            .modules()
            .retention_counts(loaded.key())
            .active_calls,
        0
    );
    assert_eq!(
        runtime
            .validate_host_borrow(token, HostBorrowKind::Unique)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(&a));
    assert!(!runtime.gc().validate_value(&b));
    assert!(!runtime.is_quarantined());
}

#[test]
fn declared_borrows_conflict_during_callbacks_and_release_after_failure() {
    let mut runtime = runtime();
    let object = root(&mut runtime);
    let invoked = Rc::new(Cell::new(0));
    let called = invoked.clone();
    runtime
        .register_host_function(HostFunction::new(
            declaration("game.write", HostPassingStyle::UniqueBorrow),
            move |context, args| {
                called.set(called.get() + 1);
                assert_eq!(
                    context
                        .runtime()
                        .invoke_host("game.read", args)
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::HostBorrowConflict
                );
                Err(HostError::new("business failure"))
            },
        ))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            declaration("game.read", HostPassingStyle::SharedBorrow),
            |_, _| Ok(Value::Unit),
        ))
        .unwrap();
    assert_eq!(
        runtime
            .invoke_host("game.write", std::slice::from_ref(&object))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(invoked.get(), 1);
    runtime.invoke_host("game.read", &[object]).unwrap();
    let scope = runtime.host_scope(&[]).unwrap();
    let token = scope
        .borrows()
        .borrow_shared(HostObjectId(1), TypeId::new(0))
        .unwrap();
    assert_eq!(
        runtime
            .invoke_host("game.write", &[Value::host_ref(token)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    runtime
        .invoke_host("game.read", &[Value::host_ref(token)])
        .unwrap();
    drop(scope);
    assert_eq!(
        runtime
            .invoke_host("game.read", &[Value::host_ref(token)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    assert_eq!(invoked.get(), 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn quarantine_does_not_block_host_scope_cleanup() {
    let mut runtime = runtime();
    let loaded = runtime
        .load_program(
            "quarantine",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let session = runtime
        .begin_execution(&loaded, runtime.execution_options())
        .unwrap();
    let value = Value::Array(
        runtime
            .alloc_array(&loaded, Ty::Builtin(BuiltinType::I32), vec![Value::I32(3)])
            .unwrap(),
    );
    let scope = runtime.host_scope(&[value]).unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    assert_eq!(
        stack
            .push(&runtime, loaded.slot(), FunctionRef::new(0), &[], None)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::EngineFault
    );
    assert!(scope.retain_values(&[Value::I32(4)]).is_err());
    assert!(
        scope
            .borrows()
            .borrow_shared(HostObjectId(2), TypeId::new(0))
            .is_err()
    );
    drop(stack);
    drop(scope);
    assert_eq!(session.host_scope_count(), 0);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    drop(session);
    assert!(runtime.execution_root().is_none());
    assert_eq!(
        runtime
            .modules()
            .retention_counts(loaded.key())
            .active_calls,
        0
    );
}

#[test]
fn callback_temporaries_survive_nested_collection_and_drop_on_error() {
    let mut runtime = runtime();
    let module = runtime
        .load_program(
            "allocation-owner",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let raw = Rc::new(RefCell::new(None));
    let saved = raw.clone();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.make", vec![], HostValueType::Unit),
            move |context, _| {
                let value = Value::Array(
                    context
                        .runtime()
                        .alloc_array(&module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
                        .unwrap(),
                );
                context
                    .retain_temporaries(std::slice::from_ref(&value))
                    .unwrap();
                context.runtime().collect_garbage().unwrap();
                assert!(context.runtime().gc().validate_value(&value));
                *saved.borrow_mut() = Some(value);
                Err(HostError::new("stop"))
            },
        ))
        .unwrap();
    assert_eq!(
        runtime.invoke_host("game.make", &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    runtime.collect_garbage().unwrap();
    assert!(!runtime.gc().validate_value(raw.borrow().as_ref().unwrap()));
}

#[test]
fn host_callbacks_observe_replayable_root_inputs() {
    let mut runtime = runtime();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let captured = observed.clone();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.sample", vec![], HostValueType::Unit),
            move |context, _| {
                captured.borrow_mut().push((
                    context.execution_time_millis().unwrap(),
                    context.next_execution_random_u64().unwrap(),
                ));
                Ok(Value::Unit)
            },
        ))
        .unwrap();
    let module = runtime
        .load_program(
            "inputs",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let mut options = runtime.execution_options();
    options.inputs.unix_time_millis = 987_654;
    options.inputs.random_seed = 52;
    for _ in 0..2 {
        let session = runtime.begin_execution(&module, options.clone()).unwrap();
        runtime.invoke_host("game.sample", &[]).unwrap();
        runtime.invoke_host("game.sample", &[]).unwrap();
        drop(session);
    }
    let records = observed.borrow();
    assert_eq!(&records[..2], &records[2..]);
    assert!(records.iter().all(|(time, _)| *time == 987_654));
    assert_ne!(records[0].1, records[1].1);
}

#[test]
fn host_trace_keeps_invocation_order_across_reentry() {
    let mut runtime = runtime();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.inner", vec![], HostValueType::I32),
            |_, _| Ok(Value::I32(2)),
        ))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.outer", vec![], HostValueType::I32),
            |context, _| {
                let inner = context.runtime().invoke_host("game.inner", &[]).unwrap();
                assert_eq!(inner, Value::I32(2));
                Ok(Value::I32(3))
            },
        ))
        .unwrap();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.fail", vec![], HostValueType::I32),
            |_, _| Err(HostError::new("expected failure")),
        ))
        .unwrap();
    let module = runtime
        .load_program(
            "trace",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            },
        )
        .unwrap();
    let mut options = runtime.execution_options();
    options.record_host_calls = true;
    options.inputs.unix_time_millis = 10;
    let session = runtime.begin_execution(&module, options).unwrap();
    assert_eq!(
        runtime.invoke_host("game.outer", &[]).unwrap(),
        Value::I32(3)
    );
    assert_eq!(
        runtime.invoke_host("game.fail", &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostCallFailure
    );
    let trace = session.trace().unwrap();
    assert_eq!(trace.code_fingerprint, module.program_fingerprint());
    assert_eq!(trace.root_identity, module.bytecode.identity);
    assert_eq!(trace.inputs.unix_time_millis, 10);
    assert_eq!(trace.dropped_host_calls, 0);
    assert_eq!(
        trace
            .host_calls
            .iter()
            .map(|call| call.symbol.as_str())
            .collect::<Vec<_>>(),
        vec!["game.outer", "game.inner", "game.fail"]
    );
    assert_eq!(trace.host_calls[0].outcome, Some(Ok(TraceValue::I32(3))));
    assert_eq!(trace.host_calls[1].outcome, Some(Ok(TraceValue::I32(2))));
    assert_eq!(
        trace.host_calls[2].outcome,
        Some(Err(RuntimeErrorKind::HostCallFailure))
    );
}
