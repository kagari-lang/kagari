use crate::test_support::native::program::ProgramFixture;
mod scoped_loans;
use crate::test_support as support;

use crate::{
    Runtime, RuntimeConfig,
    error::RuntimeErrorKind,
    host::{HostBorrowKind, HostCallContext, HostError, HostObjectId, HostSchemaEpoch},
    metadata::TypeId,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, host::HostCallback, types::Type},
    session::TraceValue,
    value::Value,
};
use kagari_bytecode::{module::BytecodeModule, program::ModuleRef};
use kagari_common::identity::map::DefinitionContext;
use kagari_types::{
    declaration::host::{
        HostCallPolicy, HostFunctionEffects, HostPassingStyle, HostReflectionPolicy,
        HostTypeOwnership, HostTypePolicy, PathAccess,
    },
    scalar::BuiltinType,
    ty::Ty,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind},
};
use support::native::{invoke::invoke, program::free_function_program};

fn runtime() -> Runtime {
    Runtime::new(RuntimeConfig {
        ..Default::default()
    })
}

fn function(name: &str, result: Type) -> FunctionDecl {
    FunctionDecl::new(name)
        .returns(result)
        .host_policy(HostCallPolicy {
            passing: vec![],
            effects: Default::default(),
        })
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
            .host_scope(&[first
                .gc()
                .alloc_tuple(vec![first.gc().alloc_host_ref(bt).unwrap()])
                .unwrap()])
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
    let context = DefinitionContext::new().unwrap();
    for with_session in [false, true] {
        let mut runtime = runtime();
        let loaded_program = runtime
            .load_program(
                "unwind",
                ProgramFixture {
                    exports: Default::default(),
                    root: ModuleRef::new(0),
                    modules: vec![BytecodeModule::default()],
                }
                .metadata(context.snapshot(), &Default::default())
                .unwrap(),
            )
            .unwrap();
        let loaded = runtime.select_program(&loaded_program).unwrap();
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
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let loaded_program = runtime
        .load_program(
            "scope",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
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
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let mut object = module.define_type("Object").unwrap();
    object
        .external(HostTypePolicy {
            ownership: HostTypeOwnership::HostRoot,
            reflection: HostReflectionPolicy::Hidden,
            path_access: PathAccess::ReadWrite,
        })
        .unwrap();
    let object = object.finish().unwrap();
    let write = module
        .define_function(
            FunctionDecl::new("write")
                .parameter("object", object.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::UniqueBorrow],
                    effects: HostFunctionEffects {
                        may_trap: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    let read = module
        .define_function(
            FunctionDecl::new("read")
                .parameter("object", object.apply([]).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::SharedBorrow],
                    effects: Default::default(),
                }),
        )
        .unwrap();
    let invoked = Arc::new(AtomicUsize::new(0));
    let called = invoked.clone();
    module
        .bind(write, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context: &HostCallContext<'_>, args: &[Value]| {
                called.fetch_add(1, Ordering::SeqCst);
                let owner = context.runtime().execution_root().unwrap();
                assert_eq!(
                    invoke(context.runtime(), &owner, 1, args)
                        .unwrap_err()
                        .kind(),
                    RuntimeErrorKind::HostBorrowConflict
                );
                Err(HostError::new("business failure"))
            })
        })
        .unwrap();
    module
        .bind(read, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|_: &HostCallContext<'_>, _: &[Value]| Ok(Value::Unit))
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let ty = runtime.types().get_by_name("game::Object").unwrap().id;
    let object = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let object = runtime.gc().alloc_host_root(object).unwrap();
    let loaded_program = runtime.load_program("borrows", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(
        invoke(&runtime, &loaded, 0, std::slice::from_ref(&object))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
    invoke(&runtime, &loaded, 1, &[object]).unwrap();
    let scope = runtime.host_scope(&[]).unwrap();
    let token = scope.borrows().borrow_shared(HostObjectId(1), ty).unwrap();
    assert_eq!(
        invoke(
            &runtime,
            &loaded,
            0,
            &[runtime.gc().alloc_host_ref(token).unwrap()]
        )
        .unwrap_err()
        .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    invoke(
        &runtime,
        &loaded,
        1,
        &[runtime.gc().alloc_host_ref(token).unwrap()],
    )
    .unwrap();
    drop(scope);
    let error = invoke(
        &runtime,
        &loaded,
        1,
        &[runtime.gc().alloc_host_ref(token).unwrap()],
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        RuntimeErrorKind::ExpiredHostBorrow,
        "{error:?}"
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[test]
fn quarantine_does_not_block_host_scope_cleanup() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let loaded_program = runtime
        .load_program(
            "quarantine",
            ProgramFixture {
                exports: Default::default(),
                root: ModuleRef::new(0),
                modules: vec![BytecodeModule::default()],
            }
            .metadata(context.snapshot(), &Default::default())
            .unwrap(),
        )
        .unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
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
        runtime
            .resources()
            .quarantine("fixture engine fault")
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
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let make = module
        .define_function(FunctionDecl::new("make").host_policy(HostCallPolicy {
            passing: vec![],
            effects: HostFunctionEffects {
                may_allocate: true,
                may_trap: true,
                ..Default::default()
            },
        }))
        .unwrap();
    module
        .bind(make, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context: &HostCallContext<'_>, _: &[Value]| {
                let owner = context.runtime().execution_root().unwrap();
                let value = Value::Array(
                    context
                        .runtime()
                        .alloc_array(&owner, Ty::Builtin(BuiltinType::I32), vec![Value::I32(7)])
                        .unwrap(),
                );
                context
                    .retain_temporaries(std::slice::from_ref(&value))
                    .unwrap();
                context.runtime().collect_garbage().unwrap();
                assert!(context.runtime().gc().validate_value(&value));
                Err(HostError::new("stop"))
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let loaded_program = runtime.load_program("temporaries", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(
        invoke(&runtime, &loaded, 0, &[]).unwrap_err().kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(runtime.gc().active_roots(), 0);
    let collection = runtime.collect_garbage().unwrap();
    assert_eq!(collection.reclaimed_objects, 1);
    assert_eq!(collection.live_objects, 0);
}

#[test]
fn host_callbacks_observe_replayable_root_inputs() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let captured = observed.clone();
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let sample = module
        .define_function(function("sample", Type::unit()))
        .unwrap();
    module
        .bind(sample, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context: &HostCallContext<'_>, _: &[Value]| {
                captured.lock().unwrap().push((
                    context.execution_time_millis().unwrap(),
                    context.next_execution_random_u64().unwrap(),
                ));
                Ok(Value::Unit)
            })
        })
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let loaded_program = runtime.load_program("inputs", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let mut options = runtime.execution_options();
    options.inputs.unix_time_millis = 987_654;
    options.inputs.random_seed = 52;
    for _ in 0..2 {
        let session = runtime.begin_execution(&loaded, options.clone()).unwrap();
        invoke(&runtime, &loaded, 0, &[]).unwrap();
        invoke(&runtime, &loaded, 0, &[]).unwrap();
        drop(session);
    }
    let records = observed.lock().unwrap();
    assert_eq!(&records[..2], &records[2..]);
    assert!(records.iter().all(|(time, _)| *time == 987_654));
    assert_ne!(records[0].1, records[1].1);
}

#[test]
fn host_trace_keeps_invocation_order_across_reentry() {
    let mut builder = ModuleBuilder::new("game", &Default::default()).unwrap();
    let inner = builder
        .define_function(function("inner", Type::i32()))
        .unwrap();
    let outer = builder
        .define_function(function("outer", Type::i32()))
        .unwrap();
    let fail = builder
        .define_function(FunctionDecl::new("fail").returns(Type::i32()).host_policy(
            HostCallPolicy {
                passing: vec![],
                effects: HostFunctionEffects {
                    may_trap: true,
                    ..Default::default()
                },
            },
        ))
        .unwrap();
    builder
        .bind(inner, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|_: &HostCallContext<'_>, _: &[Value]| Ok(Value::I32(2)))
        })
        .unwrap();
    builder
        .bind(outer, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|context: &HostCallContext<'_>, _: &[Value]| {
                let owner = context.runtime().execution_root().unwrap();
                assert_eq!(
                    invoke(context.runtime(), &owner, 0, &[]).unwrap(),
                    Value::I32(2)
                );
                Ok(Value::I32(3))
            })
        })
        .unwrap();
    builder
        .bind(fail, unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(|_: &HostCallContext<'_>, _: &[Value]| {
                Err(HostError::new("expected failure"))
            })
        })
        .unwrap();
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    let mut runtime = runtime();
    declarations.install(&mut runtime).unwrap();
    let module_program = runtime.load_program("trace", program).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let mut options = runtime.execution_options();
    options.record_host_calls = true;
    options.inputs.unix_time_millis = 10;
    let session = runtime.begin_execution(&module, options).unwrap();
    assert_eq!(invoke(&runtime, &module, 1, &[]).unwrap(), Value::I32(3));
    assert_eq!(
        invoke(&runtime, &module, 2, &[]).unwrap_err().kind(),
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
        vec!["game::outer", "game::inner", "game::fail"]
    );
    assert_eq!(trace.host_calls[0].outcome, Some(Ok(TraceValue::I32(3))));
    assert_eq!(trace.host_calls[1].outcome, Some(Ok(TraceValue::I32(2))));
    assert_eq!(
        trace.host_calls[2].outcome,
        Some(Err(RuntimeErrorKind::HostCallFailure))
    );
}
