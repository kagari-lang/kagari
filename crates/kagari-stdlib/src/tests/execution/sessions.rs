#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::tests::{
    execution::{
        common::{
            compile_logging_test_bytecode, compile_test_bytecode, logging_module,
            round_trip_bytecode, standard_runtime,
        },
        native_fixtures,
    },
    support::bytecode::reload_bytecode,
};

use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::NativeInvocationFailure},
    error::RuntimeErrorKind,
    execution::reentry::reenter,
    host::HostBorrowKind,
    module::{ExecutableModule, entry::ProgramEntry},
    resource::RuntimeLimits,
    value::Value,
};

use kagari_bytecode::program::BytecodeProgram;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_types::{language::binding, scalar::BuiltinType, ty::Ty};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn runtime() -> Runtime {
    standard_runtime(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    })
}

fn route(
    program: DefinitionMetadata<BytecodeProgram>,
    encoded: bool,
) -> DefinitionMetadata<BytecodeProgram> {
    if !encoded {
        return program;
    }
    round_trip_bytecode(program)
}

#[test]
fn host_reentry_keeps_outer_frames_results_and_borrow_scopes_alive() {
    use kagari_runtime::error::RuntimeErrorKind;
    use kagari_runtime::host::HostObjectId;
    use kagari_runtime::metadata::TypeId;
    for encoded in [false, true] {
        for jit in [false, true] {
            let module = compile_logging_test_bytecode(
                "fn main() -> i32 { val kept = [42]; print(\"outer\"); kept[0] } fn make(n: i32) -> [i32] { print(\"inner\"); [n, 8] }",
            );
            let make = module.records().modules[module.records().root.index()]
                .functions
                .iter()
                .find(|f| f.name == "make")
                .unwrap()
                .id;
            let retained = Arc::new(Mutex::new(None));
            let result = retained.clone();
            let mut runtime = runtime();
            logging_module(move |context, args| {

                    if matches!(args, [Value::Str(id)] if &*context.runtime().gc().string(*id).unwrap() == "outer") {
                        let root = context.runtime().execution_root().unwrap();
                        let token = context
                            .borrows()
                            .borrow_unique(HostObjectId(1), TypeId::new(0))
                            .unwrap();
                        assert!(
                            reenter(
                                context,
                                &root,
                                make,
                                &[Value::Bool(true)]
                            )
                            .is_err()
                        );
                        let value = reenter(
                            context,
                            &root,
                            make,
                            &[Value::I32(7)],
                        )
                        .unwrap();
                        context
                            .borrows()
                            .validate(token, HostBorrowKind::Unique)
                            .unwrap();
                        context.runtime_mut().collect_garbage().unwrap();
                        let Value::Array(id) = value.value(context.runtime().gc()).unwrap() else {
                            panic!("array result")
                        };
                        assert_eq!(
                            context.runtime().gc().array_snapshot(id).unwrap(),
                            [Value::I32(7), Value::I32(8)]
                        );
                        *result.lock().unwrap() = Some(value);
                    } else {
                        assert_eq!(context.runtime().resources().counters().current_call_depth, 4);
                        assert_eq!(
                            context
                                .borrows()
                                .borrow_shared(HostObjectId(1), TypeId::new(0))
                                .unwrap_err()
                                .kind(),
                            RuntimeErrorKind::HostBorrowConflict
                        );
                        context.runtime_mut().collect_garbage().unwrap();
                    }
                    Ok(Value::Unit)
                }).install(&mut runtime)
                .unwrap();
            let loaded =
                load_bytecode(&mut runtime, "reentry.kgr", route(module, encoded)).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut vm = runtime;
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared_module(
                    &loaded,
                    ProgramEntry::Main,
                    &prepared,
                    vm.execution_options(),
                )
            } else {
                vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            }
            .unwrap();
            assert_eq!(
                report
                    .return_value
                    .value(vm.gc())
                    .expect("retained execution result"),
                Value::I32(42)
            );
            drop(report);
            assert_eq!(vm.resources().counters().current_call_depth, 0);
            assert!(vm.execution_root().is_none());
            assert_eq!(vm.modules().retention_counts(loaded.key()).active_calls, 0);
            let mut resources = vm.host_scope(&[]).unwrap();
            let frame = resources.borrows();
            frame
                .borrow_unique(HostObjectId(1), TypeId::new(0))
                .unwrap();
            drop(frame);
            resources.runtime_mut().collect_garbage().unwrap();
            let raw = retained
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .value(resources.runtime().gc())
                .unwrap();
            assert!(resources.runtime().gc().validate_value(&raw));
            assert!(!Runtime::default().gc().validate_value(&raw));
            retained.lock().unwrap().take();
            resources.runtime_mut().collect_garbage().unwrap();
            assert!(!resources.runtime().gc().validate_value(&raw));
            assert_eq!(resources.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn host_reentry_cannot_swallow_root_termination_and_releases_borrows() {
    use kagari_runtime::host::HostObjectId;
    use kagari_runtime::metadata::TypeId;
    for cancel in [false, true] {
        for encoded in [false, true] {
            for jit in [false, true] {
                let module = compile_logging_test_bytecode(
                    "fn main() -> i32 { print(\"outer\"); 42 } fn nested() -> i32 { print(\"inner\"); 7 }",
                );
                let nested = module.records().modules[module.records().root.index()]
                    .functions
                    .iter()
                    .find(|f| f.name == "nested")
                    .unwrap()
                    .id;
                let token = CancellationToken::default();
                let cancellation = token.clone();
                let mut runtime = standard_runtime(RuntimeConfig {
                    limits: RuntimeLimits {
                        max_call_depth: if cancel { Some(256) } else { Some(2) },
                    },
                    ..Default::default()
                });
                logging_module(move |context, args| {
                    let root = context.runtime().execution_root().unwrap();
                    context.borrows().borrow_shared(HostObjectId(2), TypeId::new(0)).unwrap();
                    let temporary = Value::Array(context.runtime().alloc_array(&root, Ty::Builtin(BuiltinType::I32), vec![Value::I32(11)]).unwrap());
                    context.retain_temporaries(std::slice::from_ref(&temporary)).unwrap();
                    context.runtime_mut().collect_garbage().unwrap();
                    assert!(context.runtime().gc().validate_value(&temporary));
                    if matches!(args, [Value::Str(id)] if &*context.runtime().gc().string(*id).unwrap() == "inner") {
                        cancellation.cancel();
                    } else {
                        let error = reenter(context, &root, nested, &[]).expect_err("nested termination");
                        assert!(matches!(&error, error if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded }));
                    }
                    // Even a host deliberately swallowing a nested terminal error cannot resume.
                    Ok(Value::Unit)
                }).install(&mut runtime).unwrap();
                let loaded =
                    load_bytecode(&mut runtime, "termination.kgr", route(module, encoded)).unwrap();
                let _loaded_program = loaded;
                let loaded = runtime.select_program(&_loaded_program).unwrap();
                let mut options = runtime.execution_options();
                options.cancellation = token;

                let mut vm = runtime;
                let mut scope = vm.begin_execution(&loaded, options).unwrap();
                let scope_options = scope.runtime().execution_options();
                let scope_options = scope_options.clone();

                let prepared = native_fixtures::unsupported();
                let error = if jit {
                    scope.runtime_mut().execute_prepared_module(
                        &loaded,
                        ProgramEntry::Main,
                        &prepared,
                        scope_options.clone(),
                    )
                } else {
                    scope.runtime_mut().execute_module(
                        &loaded,
                        ProgramEntry::Main,
                        scope_options.clone(),
                    )
                }
                .unwrap_err();
                let names = error
                    .trace()
                    .unwrap()
                    .frames
                    .iter()
                    .map(|frame| frame.function_name.as_str())
                    .collect::<Vec<_>>();
                assert_eq!(
                    names,
                    if cancel {
                        vec!["log", "nested", "log", "main"]
                    } else {
                        vec!["log", "main"]
                    }
                );
                assert!(
                    matches!(&error, error if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded })
                );
                assert_eq!(scope.runtime().resources().counters().current_call_depth, 0);
                assert_eq!(scope.runtime().gc().active_roots(), 0);
                assert_eq!(scope.host_scope_count(), 0);
                drop(scope);
                let resources = vm.host_scope(&[]).unwrap();
                let frame = resources.borrows();
                frame
                    .borrow_unique(HostObjectId(2), TypeId::new(0))
                    .unwrap();
                assert!(resources.runtime().execution_root().is_none());
            }
        }
    }
}

#[test]
fn reentry_rejects_foreign_and_stale_inputs() {
    let module = compile_logging_test_bytecode(
        "fn main() -> i32 { print(\"enter\"); 42 } fn echo(value: Vec<i32>) -> Vec<i32> { value }",
    );
    let main = module.records().modules[module.records().root.index()]
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    let echo = module.records().modules[module.records().root.index()]
        .functions
        .iter()
        .find(|f| f.name == "echo")
        .unwrap()
        .id;
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let mut foreign = standard_runtime(Default::default());
    logging_module(|_, _| Ok(Value::Unit))
        .install(&mut foreign)
        .unwrap();
    let foreign_module = load_bytecode(
        &mut foreign,
        "foreign.kgr",
        compile_test_bytecode("fn main() -> i32 { 7 }"),
    )
    .unwrap();
    let _foreign_module_program = foreign_module;
    let foreign_module = foreign.select_program(&_foreign_module_program).unwrap();
    let foreign_value = Value::Array(
        foreign
            .alloc_array(&foreign_module, Ty::Builtin(BuiltinType::I32), vec![])
            .unwrap(),
    );
    assert!(foreign.gc().validate_value(&foreign_value));
    let retained_foreign_value = foreign_value;
    let mut runtime = runtime();
    logging_module(move |context, _| {
        called.fetch_add(1, Ordering::SeqCst);

        assert!(matches!(reenter(context, &foreign_module, main, &[]), Err(error) if error.kind() == RuntimeErrorKind::StaleHandle));
        if let Some(root) = context.runtime().execution_root() {
                assert!(matches!(reenter(context, &root, echo, std::slice::from_ref(&foreign_value)), Err(error) if error.kind() == RuntimeErrorKind::ModuleValidation));
                let stale = Value::Array(context.runtime().alloc_array(&root, Ty::Builtin(BuiltinType::I32), vec![]).unwrap());
                context.runtime_mut().collect_garbage().unwrap();
                assert!(matches!(reenter(context, &root, echo, &[stale]), Err(error) if error.kind() == RuntimeErrorKind::ModuleValidation));
        }
        Ok(Value::Unit)
    }).install(&mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "inputs.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(foreign.gc().validate_value(&retained_foreign_value));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(vm.gc().active_roots(), 0);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn reentry_selects_current_code_and_publication_requires_an_idle_boundary() {
    for encoded in [false, true] {
        let program = |number| {
            route(
                compile_logging_test_bytecode(&format!(
                    "fn main() -> i32 {{ print(\"enter\"); 0 }} fn value() -> i32 {{ {number} }}"
                )),
                encoded,
            )
        };
        let first = program(7);
        let function = first.records().modules[first.records().root.index()]
            .functions
            .iter()
            .find(|f| f.name == "value")
            .unwrap()
            .id;
        let versions = Arc::new(Mutex::new(Vec::<ExecutableModule>::new()));
        let observed = Arc::new(Mutex::new(Vec::new()));
        let targets = versions.clone();
        let values = observed.clone();
        let mut runtime = runtime();
        logging_module(move |context, _| {
            let versions = targets.lock().unwrap().clone();
            for version in &versions {
                let result = reenter(context, version, function, &[]);
                values
                    .lock()
                    .unwrap()
                    .push(result.unwrap().value(context.runtime().gc()).unwrap());
            }
            Ok(Value::Unit)
        })
        .install(&mut runtime)
        .unwrap();
        let old = load_bytecode(&mut runtime, "versions.kgr", first).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let mut vm = runtime;
        let mut scope = vm.begin_execution(&old, vm.execution_options()).unwrap();
        let scope_options = scope.runtime().execution_options();
        let scope_options = scope_options.clone();
        // This live session excludes mutable reload through Rust borrowing.
        assert_eq!(
            scope.runtime().select_program(&_old_program).unwrap().key(),
            old.key()
        );
        versions.lock().unwrap().push(old.clone());
        scope
            .runtime_mut()
            .execute_module(&old, ProgramEntry::Main, scope_options.clone())
            .unwrap();
        drop(scope);
        reload_bytecode(
            &mut vm,
            &_old_program,
            Some("versions.kgr".into()),
            program(9),
        )
        .unwrap();
        let new = vm.select_program(&_old_program).unwrap();
        versions.lock().unwrap().push(new.clone());
        vm.execute_module(&new, ProgramEntry::Main, vm.execution_options())
            .unwrap();
        assert_eq!(
            *observed.lock().unwrap(),
            [Value::I32(7), Value::I32(9), Value::I32(9)]
        );
    }
}

#[test]
fn reentry_trap_cleans_nested_frames_without_terminating_the_outer_call() {
    let module = compile_logging_test_bytecode(
        "fn main() -> i32 { print(\"enter\"); 42 } fn fail(n: i32) -> i32 { val kept = [n]; n + 1 }",
    );
    let fail = module.records().modules[module.records().root.index()]
        .functions
        .iter()
        .find(|f| f.name == "fail")
        .unwrap()
        .id;
    let mut runtime = runtime();
    logging_module(move |context, args| {
        let root = context.runtime().execution_root().unwrap();
        let error = reenter(context, &root, fail, &[Value::I32(i32::MAX)]).unwrap_err();
        assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::ScriptTrap));
        assert_eq!(
            context.runtime().resources().counters().current_call_depth,
            2
        );
        context.runtime_mut().collect_garbage().unwrap();
        assert_eq!(context.runtime().gc().allocated_objects(), 1);
        let [Value::Str(text)] = args else {
            panic!("outer argument")
        };
        assert_eq!(&*context.runtime().gc().string(*text).unwrap(), "enter");
        Ok(Value::Unit)
    })
    .install(&mut runtime)
    .unwrap();
    let loaded = load_bytecode(&mut runtime, "trap.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let mut vm = runtime;
    assert_eq!(
        vm.execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap()
            .return_value
            .value(vm.gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.gc().active_roots(), 0);
    // Releasing the published constant cache makes exact-zero collection detect
    // leaked frames as well as heap values after the nested trap.
    let candidate = vm
        .stage_reload_verified_program(&_loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    vm.publish_staged_reload(candidate).unwrap();
    assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn native_safepoint_reports_cancellation_at_the_current_instruction() {
    let mut runtime = runtime();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = load_bytecode(&mut runtime, "native-cancel.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let native = native_fixtures::install_i32::<42>(&runtime, &loaded, false);
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let mut session = runtime.begin_execution(&loaded, options).unwrap();
    token.cancel();
    assert!(
        matches!(session.runtime_mut().invoke_native_function(&native),
        Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(error), .. }) if error.kind() == RuntimeErrorKind::Cancelled)
    );

    drop(session);
    assert_eq!(
        runtime.invoke_native_function(&native).unwrap(),
        Value::I32(42)
    );
}

#[test]
fn cancellation_after_a_host_effect_releases_frames_and_preserves_the_effect() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime();
            let token = CancellationToken::default();
            let cancel = token.clone();
            let calls = Arc::new(AtomicUsize::new(0));
            let recorded = calls.clone();
            logging_module(move |_, _| {
                recorded.fetch_add(1, Ordering::SeqCst);
                cancel.cancel();
                Ok(Value::Unit)
            })
            .install(&mut runtime)
            .unwrap();
            let module = compile_logging_test_bytecode(
                "fn main() -> i32 { val kept = [1]; print(\"cancel\"); 42 } pub fn ready() -> i32 { 7 }",
            );
            let loaded = load_bytecode(&mut runtime, "cancel.kgr", route(module, encoded)).unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut options = runtime.execution_options();
            options.cancellation = token;
            let mut vm = runtime;
            let mut session = vm.begin_execution(&loaded, options).unwrap();
            let session_options = session.runtime().execution_options();
            let session_options = session_options.clone();
            let prepared = native_fixtures::unsupported();
            let error = if jit {
                session
                    .runtime_mut()
                    .execute_prepared_module(
                        &loaded,
                        ProgramEntry::Main,
                        &prepared,
                        session_options.clone(),
                    )
                    .unwrap_err()
            } else {
                session
                    .runtime_mut()
                    .execute_module(&loaded, ProgramEntry::Main, session_options.clone())
                    .unwrap_err()
            };
            assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::Cancelled));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(session.runtime().gc().active_roots(), 0);
            assert_eq!(
                session.runtime().resources().counters().current_call_depth,
                0
            );
            assert!(
                session
                    .runtime_mut()
                    .execute_module(&loaded, "ready", session_options.clone())
                    .is_err()
            );
            drop(session);
            vm.collect_garbage().unwrap();
            assert_eq!(vm.gc().allocated_objects(), 1, "published string constant");
            assert_eq!(
                vm.execute_module(&loaded, "ready", vm.execution_options())
                    .unwrap()
                    .return_value
                    .value(vm.gc())
                    .expect("retained execution result"),
                Value::I32(7)
            );
            let candidate = vm
                .stage_reload_verified_program(
                    &_loaded_program,
                    None,
                    loaded.verified_program().clone(),
                )
                .unwrap();
            vm.publish_staged_reload(candidate).unwrap();
            assert_eq!(vm.collect_garbage().unwrap().live_objects, 0);
            assert!(!vm.is_quarantined());
        }
    }
}

#[test]
fn staged_modules_cannot_execute_effects_through_an_ordinary_vm_entry() {
    for encoded in [false, true] {
        let program = || {
            route(
                compile_logging_test_bytecode("fn main() { print(\"forbidden\"); }"),
                encoded,
            )
        };
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = calls.clone();
        let mut runtime = runtime();
        logging_module(move |_, _| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(Value::Unit)
        })
        .install(&mut runtime)
        .unwrap();
        let old = load_bytecode(&mut runtime, "staged.kgr", program()).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let candidate = runtime
            .stage_reload_program(&_old_program, Some("staged.kgr".into()), program())
            .unwrap();
        let mut vm = runtime;
        let error = vm
            .execute_module(
                candidate.module(),
                ProgramEntry::Main,
                vm.execution_options(),
            )
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert!(vm.execution_root().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let mut session = vm.begin_candidate_initialization(&candidate).unwrap();
        let session_options = session.runtime_mut().execution_options();
        let session_options = session_options.clone();
        assert!(
            session
                .runtime_mut()
                .begin_candidate_initialization(&candidate)
                .is_err()
        );
        let error = session
            .runtime_mut()
            .execute_module(
                candidate.module(),
                ProgramEntry::Main,
                session_options.clone(),
            )
            .unwrap_err();
        assert!(
            matches!(&error, error if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(session);
        drop(candidate);
        assert_eq!(vm.modules().loaded_count(), old.members().count());
        assert!(vm.execution_root().is_none());
        assert!(!vm.is_quarantined());
    }
}

#[test]
fn host_created_err_captures_script_site_and_reentry_traps_keep_inner_origin() {
    use kagari_runtime::host::HostError;
    for encoded in [false, true] {
        let module = compile_logging_test_bytecode(
            "fn main()->i32 { print(\"entry\");42 } fn fail()->i32 {42/0}",
        );
        let fail = module.records().modules[module.records().root.index()]
            .functions
            .iter()
            .find(|f| f.name == "fail")
            .unwrap()
            .id;
        let captured = Arc::new(Mutex::new(None));
        let saved = captured.clone();
        let mut runtime = runtime();
        logging_module(move |context, _| {
            let owner = context.runtime().execution_root().unwrap();
            let ty = binding::result(
                &context.runtime().host().definitions(),
                Ty::Builtin(BuiltinType::I32),
                Ty::Builtin(BuiltinType::String),
            )
            .unwrap();
            let applied = context
                .runtime()
                .resolve_type_arguments(&owner, &[ty])
                .unwrap()
                .remove(0);
            let value = context
                .runtime()
                .make_enum_member(
                    &owner,
                    &applied,
                    "Err",
                    vec![
                        context
                            .runtime()
                            .gc()
                            .alloc_string("host failure".into())
                            .unwrap(),
                    ],
                )
                .unwrap();
            *saved.lock().unwrap() = context.runtime().result_failure(&value);
            let root = context.runtime().execution_root().unwrap();
            let error = reenter(context, &root, fail, &[]).unwrap_err();
            Err(HostError::new("nested call failed").with_trace(error.trace().unwrap().clone()))
        })
        .install(&mut runtime)
        .unwrap();
        let loaded =
            load_bytecode(&mut runtime, "host-origin.kgr", route(module, encoded)).unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let mut vm = runtime;
        let error = vm
            .execute_module(&loaded, ProgramEntry::Main, vm.execution_options())
            .unwrap_err();
        assert_eq!(
            error
                .trace()
                .unwrap()
                .frames
                .iter()
                .map(|frame| frame.function_name.as_str())
                .collect::<Vec<_>>(),
            ["fail", "log", "main"]
        );
        let saved = captured.lock().unwrap();
        let report = saved.as_ref().unwrap();
        assert_eq!(report.message, "host failure");
        assert_eq!(report.trace.frames.len(), 2);
        assert_eq!(report.trace.frames[0].function_name, "log");
        assert_eq!(report.trace.frames[1].function_name, "main");
        vm.collect_garbage().unwrap();
        assert_eq!(vm.gc().active_roots(), 0);
        assert!(vm.execution_root().is_none());
    }
}
