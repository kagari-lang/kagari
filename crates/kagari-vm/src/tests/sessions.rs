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
    let artifact = KbcArtifact::from_program(program, Default::default());
    let decoded = KbcArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    decoded.validate_for_loader(&Default::default()).unwrap();
    decoded.program
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
                    .register_host_function(HostFunction::new(standard_log(), move |_| {
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
