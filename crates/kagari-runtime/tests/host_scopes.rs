use kagari_common::host_interface::{
    HostFunctionDeclaration, HostParameter, HostPassingStyle, HostValueType,
};
use kagari_ir::bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
use kagari_runtime::{
    AbiFingerprint, CapabilitySet, HostBorrowKind, HostExposurePolicy, HostObjectId,
    HostRootHandle, HostSchemaEpoch, LanguageProfile, Runtime, RuntimeConfig, RuntimeErrorKind,
    SecurityContext, TypeId,
    host::{HostError, HostFunction},
    value::Value,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allow_host_functions: true,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn root() -> Value {
    Value::HostRoot(HostRootHandle::new(
        HostObjectId(1),
        TypeId::new(0),
        HostSchemaEpoch::new(0),
        AbiFingerprint(1),
    ))
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
fn host_scopes_keep_the_root_budget_until_all_resources_are_released() {
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
    let mut options = runtime.execution_options();
    options.resources.max_instruction_steps = Some(1);
    let session = runtime.begin_execution(&loaded, options).unwrap();
    let a = Value::Array(runtime.alloc_array(vec![Value::I32(1)]).unwrap());
    let b = Value::Array(runtime.alloc_array(vec![Value::I32(2)]).unwrap());
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
    runtime.consume_instruction_step().unwrap();
    assert_eq!(
        runtime.consume_instruction_step().unwrap_err().kind(),
        RuntimeErrorKind::ResourceLimitExceeded
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
            .invoke_host("game.write", &[root()])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(invoked.get(), 1);
    runtime.invoke_host("game.read", &[root()]).unwrap();
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
    let value = Value::Array(runtime.alloc_array(vec![Value::I32(3)]).unwrap());
    let scope = runtime.host_scope(&[value]).unwrap();
    scope
        .borrows()
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let stack = runtime.enter_execution_stack(&loaded).unwrap();
    assert_eq!(
        stack
            .push(
                loaded.slot(),
                kagari_ir::bytecode::FunctionRef::new(0),
                &[],
                None
            )
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
    let raw = Rc::new(RefCell::new(None));
    let saved = raw.clone();
    runtime
        .register_host_function(HostFunction::new(
            HostFunctionDeclaration::new("game.make", vec![], HostValueType::Unit),
            move |context, _| {
                let value =
                    Value::Array(context.runtime().alloc_array(vec![Value::I32(7)]).unwrap());
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
