#[cfg(test)]
use crate::test_support::bytecode::load_bytecode;
use crate::{
    Runtime, RuntimeConfig,
    error::{RuntimeErrorKind, fault::EngineFault},
    execution::roots::ExecutionRoots,
    host::{HostBorrowKind, HostCallContext, HostError, HostObjectId, HostSchemaEpoch},
    metadata::TypeId,
    module::managed::source::RevisionSource,
    native::{builder::ModuleBuilder, declarations::FunctionDecl, host, types::Type},
    session::TraceValue,
    test_support::{self as support, native::program::ProgramFixture},
    value::{
        EphemeralValue,
        aggregates::tuples::TupleObject,
        arrays::ArrayObject,
        carriers::{RootedValue, ScopedValue},
        host_values::EphemeralObject,
        view::ValueView,
    },
};

mod scoped_loans;

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
    let mut first = runtime();
    let mut second = runtime();
    let mut a = first.host_scope(&[]).unwrap();
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
        b.runtime()
            .validate_host_borrow(at, HostBorrowKind::Shared)
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    let foreign = {
        let Runtime {
            memory, resources, ..
        } = a.runtime_mut();
        memory.scope(resources, |cx, _| {
            let foreign = EphemeralObject::alloc(cx, EphemeralValue::HostRef(bt))?;
            let tuple = TupleObject::alloc(cx, &[foreign])?;
            ScopedValue::managed(tuple).retain(cx)
        })
    }
    .unwrap();
    assert_eq!(
        a.runtime_mut()
            .host_scope(std::slice::from_ref(&foreign))
            .err()
            .expect("foreign Host token")
            .kind(),
        RuntimeErrorKind::ExpiredHostBorrow
    );
    drop(foreign);
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
        let loaded_program = load_bytecode(
            &mut runtime,
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
        runtime.collect_garbage().unwrap();
        let baseline = runtime.gc_stats();
        let value = runtime
            .alloc_array(
                &loaded,
                Ty::Builtin(BuiltinType::I32),
                &[RootedValue::from(7i32)],
            )
            .unwrap();
        let token = Cell::new(None);
        assert!(
            catch_unwind(AssertUnwindSafe(|| {
                let run = |runtime: &mut Runtime| {
                    let mut scope = runtime.host_scope(std::slice::from_ref(&value)).unwrap();
                    let protected = scope.import(&value).unwrap();
                    drop(value);
                    token.set(Some(
                        scope
                            .borrows()
                            .borrow_unique(HostObjectId(9), TypeId::new(0))
                            .unwrap(),
                    ));
                    if with_session {
                        assert_eq!(
                            scope
                                .runtime()
                                .resources()
                                .active_session()
                                .unwrap()
                                .host_scopes
                                .borrow()
                                .len(),
                            1
                        );
                    }
                    scope.runtime_mut().collect_garbage().unwrap();
                    assert_eq!(
                        scope.runtime().gc_stats().root_registrations,
                        baseline.root_registrations
                    );
                    {
                        let Runtime {
                            memory, resources, ..
                        } = scope.runtime_mut();
                        memory.scope(resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = protected.get(slots)?.object().unwrap();
                                let array = slots.scope().downcast::<ArrayObject>(value)?;
                                assert_eq!(slots.scope().read(array, ArrayObject::len), 1);
                                assert_eq!(
                                    ArrayObject::get(slots.scope(), array, 0)?
                                        .scalar_payload()
                                        .unwrap()
                                        .1,
                                    7
                                );
                                Ok(())
                            })
                        })
                    }
                    .unwrap();
                    panic!("host unwind");
                };
                if with_session {
                    let mut session = runtime
                        .begin_execution(&loaded, runtime.execution_options())
                        .unwrap();
                    run(session.runtime_mut());
                } else {
                    run(&mut runtime);
                }
            }))
            .is_err()
        );
        assert!(runtime.execution_root().is_none());
        assert_eq!(runtime.resources().counters().current_call_depth, 0);
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
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
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
        assert!(!runtime.is_quarantined());
    }
}

#[test]
fn host_scopes_keep_root_cancellation_until_all_resources_are_released() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let loaded_program = load_bytecode(
        &mut runtime,
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let options = runtime.execution_options();

    let cancel = options.cancellation.clone();
    let mut session = runtime
        .admit_execution(
            &loaded,
            options,
            crate::session::ExecutionEntry::Program,
            RevisionSource::Current,
        )
        .unwrap();
    let a = runtime
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(1i32)],
        )
        .unwrap();
    let b = runtime
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(2i32)],
        )
        .unwrap();
    let mut outer = runtime.host_scope(std::slice::from_ref(&a)).unwrap();
    let a_view = outer.import(&a).unwrap();
    drop(a);
    let token = outer
        .borrows()
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let mut inner = outer
        .runtime_mut()
        .host_scope(std::slice::from_ref(&b))
        .unwrap();
    let b_view = inner.import(&b).unwrap();
    drop(b);
    assert_eq!(
        inner
            .runtime()
            .resources()
            .active_session()
            .unwrap()
            .host_scopes
            .borrow()
            .len(),
        2
    );
    inner.runtime_mut().collect_garbage().unwrap();
    assert_eq!(
        inner.runtime().gc_stats().root_registrations,
        baseline.root_registrations
    );
    {
        let Runtime {
            memory, resources, ..
        } = inner.runtime_mut();
        memory.scope(resources, |cx, roots| {
            ExecutionRoots::with(roots, cx, |slots| {
                for (view, expected) in [(a_view.reborrow(), 1), (b_view.reborrow(), 2)] {
                    let value = view.get(slots)?;
                    let array = slots
                        .scope()
                        .downcast::<ArrayObject>(value.object().unwrap())?;
                    let value = ArrayObject::get(slots.scope(), array, 0)?;
                    assert_eq!(value.scalar_payload().unwrap().1, expected);
                }
                Ok(())
            })
        })
    }
    .unwrap();
    session.retire(inner.runtime_mut());
    inner.runtime().resources().poll_execution().unwrap();
    cancel.cancel();
    assert_eq!(
        inner
            .runtime()
            .resources()
            .poll_execution()
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::Cancelled
    );
    assert!(inner.runtime_mut().host_scope(&[]).is_err());
    assert!(
        inner
            .borrows()
            .borrow_shared(HostObjectId(2), TypeId::new(0))
            .is_err()
    );
    drop(inner);
    assert!(outer.runtime().execution_root().is_some());
    drop(outer);
    assert!(runtime.execution_root().is_none());
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
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
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
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
        .bind(
            write,
            host::callback(
                move |context: &mut HostCallContext<'_>, args: &[ValueView<'_>]| {
                    called.fetch_add(1, Ordering::SeqCst);
                    let owner = context.runtime().execution_root().unwrap();
                    let root = context.retain(&args[0]).unwrap();
                    assert_eq!(
                        invoke(context.runtime_mut(), &owner, 1, &[root])
                            .unwrap_err()
                            .kind(),
                        RuntimeErrorKind::HostBorrowConflict
                    );
                    Err(HostError::new("business failure"))
                },
            ),
        )
        .unwrap();
    module
        .bind(
            read,
            host::callback(
                |_: &mut HostCallContext<'_>, _: &[ValueView<'_>]| Ok(ValueView::unit()),
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let ty = runtime.types().get_by_name("game::Object").unwrap().id;
    let object = runtime
        .register_host_root(HostObjectId(1), ty, HostSchemaEpoch::new(0))
        .unwrap();
    let object = runtime.host_root_value(object).unwrap();
    let loaded_program = load_bytecode(&mut runtime, "borrows", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    assert_eq!(
        invoke(&mut runtime, &loaded, 0, std::slice::from_ref(&object))
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
    let _ = invoke(&mut runtime, &loaded, 1, std::slice::from_ref(&object)).unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    let mut scope = runtime.host_scope(&[]).unwrap();
    let token = scope.borrows().borrow_shared(HostObjectId(1), ty).unwrap();
    let borrowed_value = {
        let Runtime {
            memory, resources, ..
        } = scope.runtime_mut();
        memory.scope(resources, |cx, _| {
            EphemeralObject::alloc(cx, EphemeralValue::HostRef(token))?.retain(cx)
        })
    }
    .unwrap();
    assert_eq!(
        invoke(
            scope.runtime_mut(),
            &loaded,
            0,
            std::slice::from_ref(&borrowed_value)
        )
        .unwrap_err()
        .kind(),
        RuntimeErrorKind::HostBorrowConflict
    );
    let _ = invoke(
        scope.runtime_mut(),
        &loaded,
        1,
        std::slice::from_ref(&borrowed_value),
    )
    .unwrap();
    drop(scope);
    let error = invoke(
        &mut runtime,
        &loaded,
        1,
        std::slice::from_ref(&borrowed_value),
    )
    .unwrap_err();
    assert_eq!(
        error.kind(),
        RuntimeErrorKind::ExpiredHostBorrow,
        "{error:?}"
    );
    assert_eq!(invoked.load(Ordering::SeqCst), 1);
    drop(borrowed_value);
    assert_eq!(runtime.gc_stats().root_registrations, baseline);
}

#[test]
fn quarantine_does_not_block_host_scope_cleanup() {
    let context = DefinitionContext::new().unwrap();
    let mut runtime = runtime();
    let loaded_program = load_bytecode(
        &mut runtime,
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
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats().root_registrations;
    let mut session = runtime
        .begin_execution(&loaded, runtime.execution_options())
        .unwrap();
    let value = session
        .runtime_mut()
        .alloc_array(
            &loaded,
            Ty::Builtin(BuiltinType::I32),
            &[RootedValue::from(3i32)],
        )
        .unwrap();
    let mut scope = session
        .runtime_mut()
        .host_scope(std::slice::from_ref(&value))
        .unwrap();
    drop(value);
    scope
        .borrows()
        .borrow_unique(HostObjectId(1), TypeId::new(0))
        .unwrap();
    let mut stack_activation = scope.runtime_mut().enter_execution_stack(&loaded).unwrap();
    stack_activation
        .with(|runtime, _stack| {
            assert_eq!(
                runtime
                    .resources()
                    .quarantine(EngineFault::HostInvariant("fixture engine fault"))
                    .kind(),
                RuntimeErrorKind::EngineFault
            );

            Ok(())
        })
        .unwrap();
    drop(stack_activation);
    assert!(scope.retain_values(&[ValueView::from(4i32)]).is_err());
    assert!(
        scope
            .borrows()
            .borrow_shared(HostObjectId(2), TypeId::new(0))
            .is_err()
    );

    drop(scope);
    assert_eq!(session.host_scope_count(), 0);
    assert_eq!(session.runtime().gc_stats().root_registrations, baseline);
    assert_eq!(
        session.runtime().resources().counters().current_call_depth,
        0
    );
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
fn callback_values_survive_collection_and_retire_at_their_own_boundaries() {
    let observed = Arc::new(AtomicUsize::new(0));
    let saved = observed.clone();
    let mut module = ModuleBuilder::new("game", &Default::default()).unwrap();
    let make = module
        .define_function(
            FunctionDecl::new("make")
                .parameter("fail", Type::bool())
                .returns(Type::array(Type::i32()).unwrap())
                .host_policy(HostCallPolicy {
                    passing: vec![HostPassingStyle::Owned],
                    effects: HostFunctionEffects {
                        may_allocate: true,
                        may_trap: true,
                        ..Default::default()
                    },
                }),
        )
        .unwrap();
    module
        .bind(
            make,
            host::callback(
                move |context: &mut HostCallContext<'_>, args: &[ValueView<'_>]| {
                    let owner = context.runtime().execution_root().unwrap();
                    let roots = context.runtime().gc_stats().root_registrations;
                    let value = context
                        .convert_value(&owner, |cx| {
                            let element = cx.type_for::<i32>()?;
                            cx.allocate_array(&element, &[ValueView::from(7i32)])
                        })
                        .unwrap();
                    assert_eq!(context.runtime().gc_stats().root_registrations, roots);
                    context.runtime_mut().collect_garbage().unwrap();
                    {
                        let Runtime {
                            memory, resources, ..
                        } = context.runtime_mut();
                        memory.scope(resources, |cx, roots| {
                            ExecutionRoots::with(roots, cx, |slots| {
                                let value = value.get(slots)?;
                                let object = slots
                                    .scope()
                                    .downcast::<ArrayObject>(value.object().unwrap())?;
                                assert_eq!(slots.scope().read(object, ArrayObject::len), 1);
                                assert_eq!(
                                    ArrayObject::get(slots.scope(), object, 0)?
                                        .scalar_payload()
                                        .unwrap()
                                        .1,
                                    7
                                );
                                Ok(())
                            })
                        })
                    }
                    .unwrap();
                    saved.fetch_add(1, Ordering::SeqCst);
                    if args[0].as_bool() == Some(true) {
                        Err(HostError::new("stop"))
                    } else {
                        Ok(value)
                    }
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let loaded_program = load_bytecode(&mut runtime, "temporaries", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    assert_eq!(
        invoke(&mut runtime, &loaded, 0, &[RootedValue::from(true)])
            .unwrap_err()
            .kind(),
        RuntimeErrorKind::HostCallFailure
    );
    assert_eq!(observed.load(Ordering::SeqCst), 1);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    let value = invoke(&mut runtime, &loaded, 0, &[RootedValue::from(false)]).unwrap();
    assert_eq!(observed.load(Ordering::SeqCst), 2);
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations + 1
    );
    runtime.collect_garbage().unwrap();
    runtime
        .memory
        .scope(&runtime.resources, |cx, _| {
            let object = cx.downcast::<ArrayObject>(value.get(cx)?.object().unwrap())?;
            assert_eq!(cx.read(object, ArrayObject::len), 1);
            assert_eq!(
                ArrayObject::get(cx, object, 0)?.scalar_payload().unwrap().1,
                7
            );
            Ok(())
        })
        .unwrap();
    drop(value);
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
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
        .bind(
            sample,
            host::callback(
                move |context: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
                    captured.lock().unwrap().push((
                        context.execution_time_millis().unwrap(),
                        context.next_execution_random_u64().unwrap(),
                    ));
                    Ok(ValueView::unit())
                },
            ),
        )
        .unwrap();
    let module = module.finish().unwrap();
    let program = free_function_program(&module.to_declaration());
    let mut runtime = runtime();
    module.install(&mut runtime).unwrap();
    let loaded_program = load_bytecode(&mut runtime, "inputs", program).unwrap();
    let loaded = runtime.select_program(&loaded_program).unwrap();
    let mut options = runtime.execution_options();
    options.inputs.unix_time_millis = 987_654;
    options.inputs.random_seed = 52;
    for _ in 0..2 {
        let mut session = runtime.begin_execution(&loaded, options.clone()).unwrap();
        let _ = invoke(session.runtime_mut(), &loaded, 0, &[]).unwrap();
        let _ = invoke(session.runtime_mut(), &loaded, 0, &[]).unwrap();
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
        .bind(
            inner,
            host::callback(|_: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
                Ok(ValueView::from(2))
            }),
        )
        .unwrap();
    builder
        .bind(
            outer,
            host::callback(|context: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
                let owner = context.runtime().execution_root().unwrap();
                assert_eq!(
                    invoke(context.runtime_mut(), &owner, 0, &[])
                        .unwrap()
                        .as_i32(),
                    Some(2)
                );
                Ok(ValueView::from(3))
            }),
        )
        .unwrap();
    builder
        .bind(
            fail,
            host::callback(|_: &mut HostCallContext<'_>, _: &[ValueView<'_>]| {
                Err(HostError::new("expected failure"))
            }),
        )
        .unwrap();
    let declarations = builder.finish().unwrap();
    let program = free_function_program(&declarations.to_declaration());
    let mut runtime = runtime();
    declarations.install(&mut runtime).unwrap();
    let module_program = load_bytecode(&mut runtime, "trace", program).unwrap();
    let module = runtime.select_program(&module_program).unwrap();
    let mut options = runtime.execution_options();
    options.record_host_calls = true;
    options.inputs.unix_time_millis = 10;
    let mut session = runtime.begin_execution(&module, options).unwrap();
    assert_eq!(
        invoke(session.runtime_mut(), &module, 1, &[])
            .unwrap()
            .as_i32(),
        Some(3)
    );
    assert_eq!(
        invoke(session.runtime_mut(), &module, 2, &[])
            .unwrap_err()
            .kind(),
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
