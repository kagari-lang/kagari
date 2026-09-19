use crate::{Vm, VmError, tests::common::compile_test_bytecode};
use kagari_common::{cancellation::CancellationToken, host_interface::standard_log};
use kagari_ir::bytecode::{BytecodeProgram, KbcArtifact, ModuleRef};
use kagari_runtime::{
    CapabilitySet, HostExposurePolicy, LanguageProfile, ModuleInitializationState, ResourcePolicy,
    Runtime, RuntimeConfig, RuntimeErrorKind, SecurityContext, host::HostFunction, value::Value,
};

fn runtime(limit: Option<u64>) -> Runtime {
    Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                allow_host_calls: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                host_calls: true,
                ..Default::default()
            },
        },
        host_exposure: HostExposurePolicy {
            allowed_host_functions: vec!["host.log".into()],
            ..Default::default()
        },
        resources: ResourcePolicy {
            max_instruction_steps: limit,
            ..Default::default()
        },
        ..Default::default()
    })
}

fn route(program: BytecodeProgram, encoded: bool) -> BytecodeProgram {
    if !encoded {
        return program;
    }
    let artifact = KbcArtifact::from_program(program, Default::default()).unwrap();
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    decoded.program
}

#[test]
fn host_reentry_keeps_outer_frames_results_and_borrow_scopes_alive() {
    use kagari_runtime::{HostObjectId, RuntimeErrorKind, TypeId};
    use std::{cell::RefCell, rc::Rc};
    for encoded in [false, true] {
        for jit in [false, true] {
            let module = compile_test_bytecode(
                "fn main() -> i32 { val kept = [42]; print(\"outer\"); kept[0] } fn make(n: i32) -> [i32] { print(\"inner\"); [n, 8] }",
            );
            let make = module
                .functions
                .iter()
                .find(|f| f.name == "make")
                .unwrap()
                .id;
            let retained = Rc::new(RefCell::new(None));
            let result = retained.clone();
            let mut runtime = runtime(None);
            runtime
                .register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    let runtime = context.runtime();
                    if args == [Value::Str("outer".into())] {
                        let token = context
                            .borrows()
                            .borrow_unique(HostObjectId(1), TypeId::new(0))
                            .unwrap();
                        assert!(
                            crate::reenter(
                                context,
                                &context.runtime().execution_root().unwrap(),
                                make,
                                &[Value::Bool(true)]
                            )
                            .is_err()
                        );
                        let value = crate::reenter(
                            context,
                            &context.runtime().execution_root().unwrap(),
                            make,
                            &[Value::I32(7)],
                        )
                        .unwrap();
                        context
                            .borrows()
                            .validate(token, kagari_runtime::HostBorrowKind::Unique)
                            .unwrap();
                        runtime.collect_garbage().unwrap();
                        let Value::Array(id) = value.value() else {
                            panic!("array result")
                        };
                        assert_eq!(
                            runtime.gc().array_snapshot(id).unwrap(),
                            [Value::I32(7), Value::I32(8)]
                        );
                        *result.borrow_mut() = Some(value);
                    } else {
                        assert_eq!(runtime.resources().counters().current_call_depth, 2);
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
                }))
                .unwrap();
            let loaded = runtime
                .load_program(
                    "reentry.kgr",
                    route(
                        BytecodeProgram {
                            root: ModuleRef::new(0),
                            modules: vec![module],
                        },
                        encoded,
                    ),
                )
                .unwrap();
            let mut vm = Vm::new(runtime);
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            let report = if jit {
                vm.execute_with_backend(&loaded, "main", &mut backend)
            } else {
                vm.execute(&loaded, "main")
            }
            .unwrap();
            assert_eq!(report.return_value, Value::I32(42));
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            assert!(vm.runtime().execution_root().is_none());
            assert_eq!(
                vm.runtime()
                    .modules()
                    .retention_counts(loaded.key())
                    .active_calls,
                0
            );
            let resources = vm.runtime().host_scope(&[]).unwrap();
            let frame = resources.borrows();
            frame
                .borrow_unique(HostObjectId(1), TypeId::new(0))
                .unwrap();
            vm.runtime().collect_garbage().unwrap();
            let raw = retained.borrow().as_ref().unwrap().value();
            assert!(vm.runtime().gc().validate_value(&raw));
            assert!(!Runtime::default().gc().validate_value(&raw));
            retained.borrow_mut().take();
            vm.runtime().collect_garbage().unwrap();
            assert!(!vm.runtime().gc().validate_value(&raw));
            assert_eq!(vm.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn host_reentry_cannot_swallow_root_termination_and_releases_borrows() {
    use kagari_runtime::{HostObjectId, TypeId};
    for cancel in [false, true] {
        for encoded in [false, true] {
            for jit in [false, true] {
                let module = compile_test_bytecode(
                    "fn main() -> i32 { print(\"outer\"); 42 } fn nested() -> i32 { print(\"inner\"); 7 }",
                );
                let nested = module
                    .functions
                    .iter()
                    .find(|f| f.name == "nested")
                    .unwrap()
                    .id;
                let token = CancellationToken::default();
                let cancellation = token.clone();
                let mut runtime = runtime(None);
                runtime.register_host_function(HostFunction::new(standard_log(), move |context, args| {
                    context.borrows().borrow_shared(HostObjectId(2), TypeId::new(0)).unwrap();
                    let temporary = Value::Array(context.runtime().alloc_array(vec![Value::I32(11)]).unwrap());
                    context.retain_temporaries(std::slice::from_ref(&temporary)).unwrap();
                    context.runtime().collect_garbage().unwrap();
                    assert!(context.runtime().gc().validate_value(&temporary));
                    if args == [Value::Str("inner".into())] {
                        cancellation.cancel();
                    } else {
                        let error = crate::reenter(context, &context.runtime().execution_root().unwrap(), nested, &[]).expect_err("nested termination");
                        assert!(matches!(error, VmError::RuntimeError(error) if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded }));
                    }
                    // Even a host deliberately swallowing a nested terminal error cannot resume.
                    Ok(Value::Unit)
                })).unwrap();
                let loaded = runtime
                    .load_program(
                        "termination.kgr",
                        route(
                            BytecodeProgram {
                                root: ModuleRef::new(0),
                                modules: vec![module],
                            },
                            encoded,
                        ),
                    )
                    .unwrap();
                let mut options = runtime.execution_options();
                options.cancellation = token;
                if !cancel {
                    options.resources.max_call_depth = Some(1);
                }
                let scope = runtime.begin_execution(&loaded, options).unwrap();
                let mut vm = Vm::new(runtime);
                let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
                let error = if jit {
                    vm.execute_with_backend(&loaded, "main", &mut backend)
                } else {
                    vm.execute(&loaded, "main")
                }
                .unwrap_err();
                assert!(
                    matches!(error, VmError::RuntimeError(error) if error.kind() == if cancel { RuntimeErrorKind::Cancelled } else { RuntimeErrorKind::ResourceLimitExceeded })
                );
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(scope.host_scope_count(), 0);
                drop(scope);
                let resources = vm.runtime().host_scope(&[]).unwrap();
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
fn reentry_rejects_uninitialized_foreign_and_stale_inputs() {
    use std::{cell::Cell, rc::Rc};
    let mut module = compile_test_bytecode(
        "fn main() -> i32 { print(\"enter\"); 42 } fn echo(value: [i32]) -> [i32] { value }",
    );
    let main = module
        .functions
        .iter()
        .find(|f| f.name == "main")
        .unwrap()
        .id;
    let echo = module
        .functions
        .iter()
        .find(|f| f.name == "echo")
        .unwrap()
        .id;
    module.module_init = Some(main);
    let calls = Rc::new(Cell::new(0));
    let called = calls.clone();
    let mut foreign = Runtime::default();
    let foreign_module = foreign
        .load_program(
            "foreign.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![compile_test_bytecode("fn main() -> i32 { 7 }")],
            },
        )
        .unwrap();
    let foreign_value = Value::Array(foreign.alloc_array(vec![]).unwrap());
    let mut runtime = runtime(None);
    runtime.register_host_function(HostFunction::new(standard_log(), move |context, _| {
        called.set(called.get() + 1);
        let runtime = context.runtime();
        assert!(matches!(crate::reenter(context, &foreign_module, main, &[]), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
        if let Some(root) = runtime.execution_root() {
            if runtime.module_instance_snapshot(&root).unwrap().state == ModuleInitializationState::Initializing {
                assert!(matches!(crate::reenter(context, &root, echo, &[Value::Array(runtime.alloc_array(vec![]).unwrap())]), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
            } else {
                assert!(foreign.gc().validate_value(&foreign_value));
                assert!(matches!(crate::reenter(context, &root, echo, std::slice::from_ref(&foreign_value)), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
                let stale = Value::Array(runtime.alloc_array(vec![]).unwrap());
                runtime.collect_garbage().unwrap();
                assert!(matches!(crate::reenter(context, &root, echo, &[stale]), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
            }
        }
        Ok(Value::Unit)
    })).unwrap();
    // A direct host call supplies a context, but cannot create a script root implicitly.
    runtime
        .invoke_host("host.log", &[Value::Str("enter".into())])
        .unwrap();
    let loaded = runtime
        .load_program(
            "inputs.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(calls.get(), 3);
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn reentry_uses_the_root_version_and_rejects_other_epochs() {
    use std::{cell::RefCell, rc::Rc};
    for encoded in [false, true] {
        let program = |number| {
            route(
                BytecodeProgram {
                    root: ModuleRef::new(0),
                    modules: vec![compile_test_bytecode(&format!(
                        "fn main() -> i32 {{ print(\"enter\"); 0 }} fn value() -> i32 {{ {number} }}"
                    ))],
                },
                encoded,
            )
        };
        let first = program(7);
        let function = first.modules[0]
            .functions
            .iter()
            .find(|f| f.name == "value")
            .unwrap()
            .id;
        let versions = Rc::new(RefCell::new(Vec::<kagari_runtime::LoadedModule>::new()));
        let observed = Rc::new(RefCell::new(Vec::new()));
        let targets = versions.clone();
        let values = observed.clone();
        let mut runtime = runtime(None);
        runtime.register_host_function(HostFunction::new(standard_log(), move |context, _| {
            let root = context.runtime().execution_root().unwrap();
            for version in targets.borrow().iter() {
                let result = crate::reenter(context, version, function, &[]);
                if version.key() == root.key() {
                    values.borrow_mut().push(result.unwrap().value());
                } else {
                    assert!(matches!(result, Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::ModuleValidation));
                }
            }
            Ok(Value::Unit)
        })).unwrap();
        let old = runtime.load_program("versions.kgr", first).unwrap();
        let mut vm = Vm::new(runtime);
        vm.execute_module(&old).unwrap();
        let scope = vm
            .runtime()
            .begin_execution(&old, vm.runtime().execution_options())
            .unwrap();
        let new = vm
            .runtime_mut()
            .reload_program(&old, "versions.kgr", program(9))
            .unwrap();
        versions.borrow_mut().extend([old.clone(), new.clone()]);
        vm.execute(&old, "main").unwrap();
        drop(scope);
        vm.execute(&new, "main").unwrap();
        assert_eq!(*observed.borrow(), [Value::I32(7), Value::I32(9)]);
    }
}

#[test]
fn reentry_trap_cleans_nested_frames_without_terminating_the_outer_call() {
    let module = compile_test_bytecode(
        "fn main() -> i32 { print(\"enter\"); 42 } fn fail(n: i32) -> i32 { val kept = [n]; n + 1 }",
    );
    let fail = module
        .functions
        .iter()
        .find(|f| f.name == "fail")
        .unwrap()
        .id;
    let mut runtime = runtime(None);
    runtime.register_host_function(HostFunction::new(standard_log(), move |context, _| {
        let root = context.runtime().execution_root().unwrap();
        let error = crate::reenter(context, &root, fail, &[Value::I32(i32::MAX)]).unwrap_err();
        assert!(matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ScriptTrap));
        assert_eq!(context.runtime().resources().counters().current_call_depth, 1);
        context.runtime().collect_garbage().unwrap();
        assert_eq!(context.runtime().gc().allocated_objects(), 0);
        Ok(Value::Unit)
    })).unwrap();
    let loaded = runtime
        .load_program(
            "trap.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap();
    let mut vm = Vm::new(runtime);
    assert_eq!(
        vm.execute(&loaded, "main").unwrap().return_value,
        Value::I32(42)
    );
    assert_eq!(vm.runtime().gc().active_roots(), 0);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn native_safepoint_reports_cancellation_before_charging_the_instruction() {
    use kagari_runtime::{BackendFunctionInput, BackendInvocationError, CodegenBackend};
    let mut runtime = runtime(None);
    let module = compile_test_bytecode("fn main() -> i32 { 42 }");
    let function = module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .unwrap()
        .id;
    let loaded = runtime
        .load_program(
            "native-cancel.kgr",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![module],
            },
        )
        .unwrap();
    let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
    let artifact = backend
        .compile_function(BackendFunctionInput::new(&loaded, function).unwrap())
        .unwrap();
    let token = CancellationToken::default();
    let mut options = runtime.execution_options();
    options.cancellation = token.clone();
    let session = runtime.begin_execution(&loaded, options).unwrap();
    token.cancel();
    assert!(
        matches!(backend.invoke_compiled_scalar(&artifact, &runtime),
        Err(BackendInvocationError::RuntimeFailure(error)) if error.kind() == RuntimeErrorKind::Cancelled)
    );
    assert_eq!(session.counters().instruction_steps, 0);
    drop(session);
    assert_eq!(
        backend.invoke_compiled_scalar(&artifact, &runtime).unwrap(),
        Value::I32(42)
    );
}

#[test]
fn initialization_and_entry_share_one_budget_and_next_root_gets_a_fresh_budget() {
    for encoded in [false, true] {
        for jit in [false, true] {
            let mut runtime = runtime(Some(3));
            let mut module =
                compile_test_bytecode("fn init() -> i32 { 5 } fn main() -> i32 { 42 }");
            module.module_init = Some(
                module
                    .functions
                    .iter()
                    .find(|function| function.name == "init")
                    .unwrap()
                    .id,
            );
            let loaded = runtime
                .load_program(
                    "session.kgr",
                    route(
                        BytecodeProgram {
                            root: ModuleRef::new(0),
                            modules: vec![module],
                        },
                        encoded,
                    ),
                )
                .unwrap();
            let mut vm = Vm::new(runtime);
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            let error = if jit {
                vm.execute_with_backend(&loaded, "main", &mut backend)
                    .unwrap_err()
            } else {
                vm.execute(&loaded, "main").unwrap_err()
            };
            assert!(
                matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ResourceLimitExceeded)
            );
            assert_eq!(vm.runtime().resources().counters().instruction_steps, 3);
            assert_eq!(
                vm.runtime()
                    .module_instance_snapshot(&loaded)
                    .unwrap()
                    .state,
                ModuleInitializationState::Initialized
            );
            assert_eq!(vm.runtime().gc().active_roots(), 0);
            assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
            let report = if jit {
                vm.execute_with_backend(&loaded, "main", &mut backend)
                    .unwrap()
            } else {
                vm.execute(&loaded, "main").unwrap()
            };
            assert_eq!(report.return_value, Value::I32(42));
            assert_eq!(vm.runtime().resources().counters().instruction_steps, 5);
            assert_eq!(
                vm.runtime()
                    .modules()
                    .retention_counts(loaded.key())
                    .active_calls,
                0
            );
        }
    }
}

#[test]
fn cancellation_after_a_host_effect_releases_frames_and_preserves_the_effect() {
    use std::{cell::Cell, rc::Rc};
    for initializing in [false, true] {
        for encoded in [false, true] {
            for jit in [false, true] {
                let mut runtime = runtime(None);
                let token = CancellationToken::default();
                let cancel = token.clone();
                let calls = Rc::new(Cell::new(0));
                let recorded = calls.clone();
                runtime
                    .register_host_function(HostFunction::new(standard_log(), move |_, _| {
                        recorded.set(recorded.get() + 1);
                        cancel.cancel();
                        Ok(Value::Unit)
                    }))
                    .unwrap();
                let mut module = compile_test_bytecode(
                    "fn main() -> i32 { val kept = [1]; print(\"cancel\"); 42 } fn ready() -> i32 { 7 }",
                );
                if initializing {
                    module.module_init = Some(
                        module
                            .functions
                            .iter()
                            .find(|function| function.name == "main")
                            .unwrap()
                            .id,
                    );
                }
                let loaded = runtime
                    .load_program(
                        "cancel.kgr",
                        route(
                            BytecodeProgram {
                                root: ModuleRef::new(0),
                                modules: vec![module],
                            },
                            encoded,
                        ),
                    )
                    .unwrap();
                let mut options = runtime.execution_options();
                options.cancellation = token;
                let session = runtime.begin_execution(&loaded, options).unwrap();
                let mut vm = Vm::new(runtime);
                let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
                let error = if jit {
                    vm.execute_with_backend(&loaded, "main", &mut backend)
                        .unwrap_err()
                } else {
                    vm.execute(&loaded, "main").unwrap_err()
                };
                assert!(
                    matches!(error, VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::Cancelled)
                );
                assert_eq!(calls.get(), 1);
                assert_eq!(vm.runtime().gc().active_roots(), 0);
                assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
                assert!(vm.execute(&loaded, "ready").is_err());
                drop(session);
                vm.runtime().collect_garbage().unwrap();
                assert_eq!(vm.runtime().gc().allocated_objects(), 0);
                if initializing {
                    assert_eq!(
                        vm.runtime()
                            .module_instance_snapshot(&loaded)
                            .unwrap()
                            .state,
                        ModuleInitializationState::Failed
                    );
                    assert!(
                        matches!(vm.execute(&loaded, "ready"), Err(VmError::RuntimeError(error)) if error.kind() == RuntimeErrorKind::Cancelled)
                    );
                    assert_eq!(calls.get(), 1);
                    let healthy = vm
                        .runtime_mut()
                        .load_program(
                            "healthy.kgr",
                            BytecodeProgram {
                                root: ModuleRef::new(0),
                                modules: vec![compile_test_bytecode("fn main() -> i32 { 7 }")],
                            },
                        )
                        .unwrap();
                    assert_eq!(
                        vm.execute(&healthy, "main").unwrap().return_value,
                        Value::I32(7)
                    );
                } else {
                    assert_eq!(
                        vm.execute(&loaded, "ready").unwrap().return_value,
                        Value::I32(7)
                    );
                }
                assert!(!vm.runtime().is_quarantined());
            }
        }
    }
}
