use crate::tests::common::logging_module;
use crate::{
    error::VmError,
    reentry::reenter,
    tests::{
        common::{
            compile_logging_test_bytecode, compile_test_bytecode, round_trip_bytecode,
            standard_runtime,
        },
        native_fixtures,
    },
    vm::{ReloadError, Vm},
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::{cancellation::CancellationToken, identity::metadata::DefinitionMetadata};
use kagari_runtime::{
    Runtime, RuntimeConfig,
    backend::{BackendInvocationError, native::NativeInvocationFailure},
    error::RuntimeErrorKind,
    host::HostBorrowKind,
    module::{ExecutableModule, entry::ProgramEntry},
    native::host::HostCallback,
    reload::ReloadValidationError,
    resource::RuntimeLimits,
    value::Value,
};
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
    use kagari_runtime::{error::RuntimeErrorKind, host::HostObjectId, metadata::TypeId};
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
            logging_module(unsafe {
// SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
HostCallback::new(move |context, args| {
                    let runtime = context.runtime();
                    if matches!(args, [Value::Str(id)] if &*runtime.gc().string_transport(*id).unwrap() == "outer") {
                        let token = context
                            .borrows()
                            .borrow_unique(HostObjectId(1), TypeId::new(0))
                            .unwrap();
                        assert!(
                            reenter(
                                context,
                                &context.runtime().execution_root().unwrap(),
                                make,
                                &[Value::Bool(true)]
                            )
                            .is_err()
                        );
                        let value = reenter(
                            context,
                            &context.runtime().execution_root().unwrap(),
                            make,
                            &[Value::I32(7)],
                        )
                        .unwrap();
                        context
                            .borrows()
                            .validate(token, HostBorrowKind::Unique)
                            .unwrap();
                        runtime.collect_garbage().unwrap();
                        let Value::Array(id) = value.value_transport(runtime.gc()).unwrap() else {
                            panic!("array result")
                        };
                        assert_eq!(
                            runtime.gc().array_snapshot_transport(id).unwrap(),
                            [Value::I32(7), Value::I32(8)]
                        );
                        *result.lock().unwrap() = Some(value);
                    } else {
                        assert_eq!(runtime.resources().counters().current_call_depth, 4);
                        assert_eq!(
                            context
                                .borrows()
                                .borrow_shared(HostObjectId(1), TypeId::new(0))
                                .unwrap_err()
                                .kind(),
                            RuntimeErrorKind::HostBorrowConflict
                        );
                        runtime.collect_garbage().unwrap();
                    }
                    Ok(Value::Unit)
                })
}).install(&mut runtime)
                .unwrap();
            let loaded = runtime
                .load_program("reentry.kgr", route(module, encoded))
                .unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let vm = Vm::new(runtime);
            let prepared = native_fixtures::unsupported();
            let report = if jit {
                vm.execute_prepared(&loaded, ProgramEntry::Main, &prepared)
            } else {
                vm.execute(&loaded, ProgramEntry::Main)
            }
            .unwrap();
            assert_eq!(
                report
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .expect("retained execution result"),
                Value::I32(42)
            );
            drop(report);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(vm.runtime().execution_root().is_none());
            assert_eq!(
                vm.runtime()
                    .modules()
                    .retention_counts(loaded.key())
                    .active_calls,
                0
            );
            let resources = unsafe {
                // SAFETY: Empty inputs; this guard owns only host borrow/session leases.
                vm.runtime().host_scope_transport(&[])
            }
            .unwrap();
            let frame = resources.borrows();
            frame
                .borrow_unique(HostObjectId(1), TypeId::new(0))
                .unwrap();
            vm.runtime().collect_garbage().unwrap();
            let raw = unsafe {
                retained
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .value_transport(vm.runtime().gc())
            }
            .unwrap();
            assert!(unsafe { vm.runtime().gc().validate_value_transport(&raw) });
            assert!(!unsafe { Runtime::default().gc().validate_value_transport(&raw) });
            retained.lock().unwrap().take();
            vm.runtime().collect_garbage().unwrap();
            assert!(!unsafe { vm.runtime().gc().validate_value_transport(&raw) });
            assert_eq!(vm.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn host_reentry_cannot_swallow_root_termination_and_releases_borrows() {
    use kagari_runtime::{host::HostObjectId, metadata::TypeId};
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
                logging_module(unsafe {
// SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
HostCallback::new(move |context, args| {
                    context.borrows().borrow_shared(HostObjectId(2), TypeId::new(0)).unwrap();
                    let temporary = Value::Array(context.runtime().alloc_array_transport(&context.runtime().execution_root().unwrap(), Ty::Builtin(BuiltinType::I32), vec![Value::I32(11)]).unwrap());
                    context.retain_temporaries(std::slice::from_ref(&temporary)).unwrap();
                    context.runtime().collect_garbage().unwrap();
                    assert!(context.runtime().gc().validate_value_transport(&temporary));
                    if matches!(args, [Value::Str(id)] if &*context.runtime().gc().string_transport(*id).unwrap() == "inner") {
                        cancellation.cancel();
                    } else {
                        let error = reenter(context, &context.runtime().execution_root().unwrap(), nested, &[]).expect_err("nested termination");
                        assert!(matches!(error, VmError::RuntimeError(error) if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded }));
                    }
                    // Even a host deliberately swallowing a nested terminal error cannot resume.
                    Ok(Value::Unit)
                })
}).install(&mut runtime).unwrap();
                let loaded = runtime
                    .load_program("termination.kgr", route(module, encoded))
                    .unwrap();
                let _loaded_program = loaded;
                let loaded = runtime.select_program(&_loaded_program).unwrap();
                let mut options = runtime.execution_options();
                options.cancellation = token;

                let vm = Vm::new(runtime);
                let scope = vm.runtime().begin_execution(&loaded, options).unwrap();

                let prepared = native_fixtures::unsupported();
                let error = if jit {
                    vm.execute_prepared(&loaded, ProgramEntry::Main, &prepared)
                } else {
                    vm.execute(&loaded, ProgramEntry::Main)
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
                    matches!(error, VmError::RuntimeError(error) if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded })
                );
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(scope.host_scope_count(), 0);
                drop(scope);
                let resources = unsafe {
                    // SAFETY: Empty inputs; this guard owns only host borrow/session leases.
                    vm.runtime().host_scope_transport(&[])
                }
                .unwrap();
                let frame = resources.borrows();
                frame
                    .borrow_unique(HostObjectId(2), TypeId::new(0))
                    .unwrap();
                assert!(vm.runtime().execution_root().is_none());
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
    logging_module(unsafe {
        // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
        HostCallback::new(|_, _| Ok(Value::Unit))
    })
    .install(&mut foreign)
    .unwrap();
    let foreign_module = foreign
        .load_program(
            "foreign.kgr",
            compile_test_bytecode("fn main() -> i32 { 7 }"),
        )
        .unwrap();
    let _foreign_module_program = foreign_module;
    let foreign_module = foreign.select_program(&_foreign_module_program).unwrap();
    let foreign_value = Value::Array(
        unsafe {
            foreign.alloc_array_transport(&foreign_module, Ty::Builtin(BuiltinType::I32), vec![])
        }
        .unwrap(),
    );
    assert!(unsafe { foreign.gc().validate_value_transport(&foreign_value) });
    let retained_foreign_value = foreign_value;
    let mut runtime = runtime();
    logging_module(unsafe {
// SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
HostCallback::new(move |context, _| {
        called.fetch_add(1, Ordering::SeqCst);
        let runtime = context.runtime();
        assert!(matches!(reenter(context, &foreign_module, main, &[]), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::StaleHandle));
        if let Some(root) = runtime.execution_root() {
                assert!(matches!(reenter(context, &root, echo, std::slice::from_ref(&foreign_value)), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
                let stale = Value::Array(runtime.alloc_array_transport(&root, Ty::Builtin(BuiltinType::I32), vec![]).unwrap());
                runtime.collect_garbage().unwrap();
                assert!(matches!(reenter(context, &root, echo, &[stale]), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
        }
        Ok(Value::Unit)
    })
}).install(&mut runtime).unwrap();
    let loaded = runtime.load_program("inputs.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert!(unsafe {
        foreign
            .gc()
            .validate_value_transport(&retained_foreign_value)
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
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
        logging_module(unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context, _| {
                let versions = targets.lock().unwrap().clone();
                for version in &versions {
                    let result = reenter(context, version, function, &[]);
                    values.lock().unwrap().push(
                        result
                            .unwrap()
                            .inspect(context.runtime(), |view| view.as_i32().map(Value::I32))
                            .unwrap(),
                    );
                }
                Ok(Value::Unit)
            })
        })
        .install(&mut runtime)
        .unwrap();
        let old = runtime.load_program("versions.kgr", first).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let vm = Vm::new(runtime);
        let scope = vm
            .runtime()
            .begin_execution(&old, vm.runtime().execution_options())
            .unwrap();
        let error = vm
            .reload_program(&_old_program, Some("versions.kgr".into()), program(9))
            .unwrap_err();
        assert!(
            matches!(error, ReloadError::Validation(ReloadValidationError::Runtime(error))
            if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(
            vm.runtime().select_program(&_old_program).unwrap().key(),
            old.key()
        );
        versions.lock().unwrap().push(old.clone());
        vm.execute(&old, ProgramEntry::Main).unwrap();
        drop(scope);
        vm.reload_program(&_old_program, Some("versions.kgr".into()), program(9))
            .unwrap();
        let new = vm.runtime().select_program(&_old_program).unwrap();
        versions.lock().unwrap().push(new.clone());
        vm.execute(&new, ProgramEntry::Main).unwrap();
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
    logging_module(unsafe {
// SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
HostCallback::new(move |context, args| {
        let root = context.runtime().execution_root().unwrap();
        let error = reenter(context, &root, fail, &[Value::I32(i32::MAX)]).unwrap_err();
        assert!(matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ScriptTrap));
        assert_eq!(context.runtime().resources().counters().current_call_depth, 2);
        context.runtime().collect_garbage().unwrap();
        assert_eq!(context.runtime().gc().allocated_objects(), 1);
        let [Value::Str(text)] = args else { panic!("outer argument") };
        assert_eq!(&*context.runtime().gc().string_transport(*text).unwrap(), "enter");
        Ok(Value::Unit)
    })
}).install(&mut runtime).unwrap();
    let loaded = runtime.load_program("trap.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, ProgramEntry::Main)
            .unwrap()
            .return_value
            .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    // Releasing the published constant cache makes exact-zero collection detect
    // leaked frames as well as heap values after the nested trap.
    let candidate = vm
        .runtime()
        .stage_reload_verified_program(&_loaded_program, None, loaded.verified_program().clone())
        .unwrap();
    vm.runtime().publish_staged_reload(candidate).unwrap();
    assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn native_safepoint_reports_cancellation_at_the_current_instruction() {
    let mut runtime = runtime();
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let loaded = runtime.load_program("native-cancel.kgr", module).unwrap();
    let _loaded_program = loaded;
    let loaded = runtime.select_program(&_loaded_program).unwrap();
    let native = native_fixtures::install_i32::<42>(&runtime, &loaded, false);
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let session = runtime.begin_execution(&loaded, options).unwrap();
    token.cancel();
    assert!(matches!(runtime.invoke_native_function(&native),
        Err(NativeInvocationFailure { error: BackendInvocationError::RuntimeFailure(error), .. }) if error.kind() == RuntimeErrorKind::Cancelled));

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
            logging_module(unsafe {
                // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
                HostCallback::new(move |_, _| {
                    recorded.fetch_add(1, Ordering::SeqCst);
                    cancel.cancel();
                    Ok(Value::Unit)
                })
            })
            .install(&mut runtime)
            .unwrap();
            let module = compile_logging_test_bytecode(
                "fn main() -> i32 { val kept = [1]; print(\"cancel\"); 42 } pub fn ready() -> i32 { 7 }",
            );
            let loaded = runtime
                .load_program("cancel.kgr", route(module, encoded))
                .unwrap();
            let _loaded_program = loaded;
            let loaded = runtime.select_program(&_loaded_program).unwrap();
            let mut options = runtime.execution_options();
            options.cancellation = token;
            let vm = Vm::new(runtime);
            let session = vm.runtime().begin_execution(&loaded, options).unwrap();
            let prepared = native_fixtures::unsupported();
            let error = if jit {
                vm.execute_prepared(&loaded, ProgramEntry::Main, &prepared)
                    .unwrap_err()
            } else {
                vm.execute(&loaded, ProgramEntry::Main).unwrap_err()
            };
            assert!(
                matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(vm.execute(&loaded, "ready").is_err());
            drop(session);
            vm.runtime().collect_garbage().unwrap();
            assert_eq!(
                vm.runtime().gc().allocated_objects(),
                1,
                "published string constant"
            );
            assert_eq!(
                vm.execute(&loaded, "ready")
                    .unwrap()
                    .return_value
                    .inspect(vm.runtime(), |view| view.as_i32().map(Value::I32))
                    .expect("retained execution result"),
                Value::I32(7)
            );
            let candidate = vm
                .runtime()
                .stage_reload_verified_program(
                    &_loaded_program,
                    None,
                    loaded.verified_program().clone(),
                )
                .unwrap();
            vm.runtime().publish_staged_reload(candidate).unwrap();
            assert_eq!(vm.runtime().collect_garbage().unwrap().live_objects, 0);
            assert!(!vm.runtime().is_quarantined());
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
        logging_module(unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |_, _| {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(Value::Unit)
            })
        })
        .install(&mut runtime)
        .unwrap();
        let old = runtime.load_program("staged.kgr", program()).unwrap();
        let _old_program = old;
        let old = runtime.select_program(&_old_program).unwrap();
        let candidate = runtime
            .stage_reload_program(&_old_program, Some("staged.kgr".into()), program())
            .unwrap();
        let vm = Vm::new(runtime);
        let error = vm
            .execute(candidate.module(), ProgramEntry::Main)
            .unwrap_err();
        assert!(
            matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert!(vm.runtime().execution_root().is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let session = vm
            .runtime()
            .begin_candidate_initialization(&candidate)
            .unwrap();
        assert!(
            vm.runtime()
                .begin_candidate_initialization(&candidate)
                .is_err()
        );
        let error = vm
            .execute(candidate.module(), ProgramEntry::Main)
            .unwrap_err();
        assert!(
            matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ExecutionPhaseViolation)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        drop(session);
        drop(candidate);
        assert_eq!(vm.runtime().modules().loaded_count(), old.members().count());
        assert!(vm.runtime().execution_root().is_none());
        assert!(!vm.runtime().is_quarantined());
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
        logging_module(unsafe {
            // SAFETY: The host frame protects inputs; escaped managed values are retained and results transfer immediately.
            HostCallback::new(move |context, _| {
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
                    .make_enum_member_transport(
                        &owner,
                        &applied,
                        "Err",
                        vec![
                            context
                                .runtime()
                                .gc()
                                .alloc_string_transport("host failure".into())
                                .unwrap(),
                        ],
                    )
                    .unwrap();
                *saved.lock().unwrap() = context.runtime().result_failure_transport(&value);
                let root = context.runtime().execution_root().unwrap();
                let error = reenter(context, &root, fail, &[]).unwrap_err();
                Err(HostError::new("nested call failed").with_trace(error.trace().unwrap().clone()))
            })
        })
        .install(&mut runtime)
        .unwrap();
        let loaded = runtime
            .load_program("host-origin.kgr", route(module, encoded))
            .unwrap();
        let _loaded_program = loaded;
        let loaded = runtime.select_program(&_loaded_program).unwrap();
        let vm = Vm::new(runtime);
        let error = vm.execute(&loaded, ProgramEntry::Main).unwrap_err();
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
        vm.runtime().collect_garbage().unwrap();
        assert_eq!(vm.runtime().gc().active_roots(), 0);
        assert!(vm.runtime().execution_root().is_none());
    }
}
