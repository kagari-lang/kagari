use std::{ffi::c_void, rc::Rc};

use kagari_abi::{
    ids::FunctionRef,
    native::{
        BackendId, BackendTarget, ExecutableEntryPoint, ExecutableFunctionArtifact,
        NativeCodeOwner, NativeCompilationProduct,
    },
    native_call::{JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK, JitCompiledFunction, JitValue},
};
use kagari_bytecode::{BytecodeModule, BytecodeProgram, ModuleRef};
use kagari_common::SourceFile;
use kagari_compiler::{MirLoweringOptions, bytecode::lower_to_bytecode, lower_to_mir};
use kagari_hir::analyze_source;
use kagari_runtime::{
    CapabilitySet, LanguageProfile, LoadedModule, ResourcePolicy, Runtime, RuntimeConfig,
    RuntimeErrorKind, SecurityContext, jit_abi::jit_consume_instruction_step, value::Value,
};
use kagari_vm::{JitExecutionStatus, PreparedNativeEntry, Vm, VmError};

fn compile(source: &str, optimize: bool) -> BytecodeModule {
    let source = SourceFile::new("test.kgr", source);
    let checked = analyze_source(&source, Default::default())
        .into_codegen()
        .unwrap();
    let mir = lower_to_mir(
        &checked,
        &MirLoweringOptions {
            optimization: optimize.then(Default::default),
            ..Default::default()
        },
    )
    .unwrap();
    lower_to_bytecode(&mir).unwrap()
}
fn setup(bytecode: BytecodeModule, limit: Option<u64>) -> (Vm, LoadedModule) {
    let mut runtime = Runtime::new(RuntimeConfig {
        security: SecurityContext {
            profile: LanguageProfile {
                allow_jit: true,
                ..Default::default()
            },
            capabilities: CapabilitySet {
                jit: true,
                ..Default::default()
            },
        },
        resources: ResourcePolicy {
            max_instruction_steps: limit,
            ..Default::default()
        },
        ..Default::default()
    });
    let loaded = runtime
        .load_program(
            "test",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![bytecode],
            },
        )
        .unwrap();
    (Vm::new(runtime), loaded)
}
#[derive(Debug)]
struct StaticCode;
impl NativeCodeOwner for StaticCode {}
unsafe extern "C" fn native_unit(runtime: *const c_void, result: *mut JitValue) -> i32 {
    for offset in 0..2 {
        let status = unsafe { jit_consume_instruction_step(runtime.cast(), offset) };
        if status != JIT_STATUS_OK {
            return status;
        }
    }
    unsafe {
        result.write(JitValue::unit());
    }
    JIT_STATUS_OK
}
unsafe extern "C" fn native_trap(runtime: *const c_void, _: *mut JitValue) -> i32 {
    let status = unsafe { jit_consume_instruction_step(runtime.cast(), 0) };
    if status == JIT_STATUS_OK {
        JIT_STATUS_INTEGER_OVERFLOW
    } else {
        status
    }
}
fn prepared(vm: &Vm, module: &LoadedModule, entry: JitCompiledFunction) -> PreparedNativeEntry {
    let mut artifact = ExecutableFunctionArtifact::new(
        BackendId::new("fixture"),
        BackendTarget::new("host", usize::BITS as u8),
        FunctionRef::new(0),
    );
    artifact.entry = ExecutableEntryPoint::Native {
        symbol: "fixture".into(),
        address: entry as usize,
    };
    let product = Rc::new(NativeCompilationProduct {
        artifact,
        owner: Rc::new(StaticCode),
    });
    // Static ABI fixture; trap probe intentionally exercises failure propagation.
    PreparedNativeEntry::Native(
        unsafe { vm.runtime().install_native_function(module, product) }.unwrap(),
    )
}

#[test]
fn prepared_native_execution_reports_the_installed_descriptor_and_cleans_frames() {
    let (mut vm, module) = setup(compile("fn main() {}", false), None);
    assert_eq!(module.bytecode.functions[0].instructions.len(), 2);
    let preparation = prepared(&vm, &module, native_unit);
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    assert_eq!(report.return_value, Value::Unit);
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert_eq!(jit.artifact.unwrap().function, FunctionRef::new(0));
    assert!(jit.diagnostics.is_empty());
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 2);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn unsupported_preparation_runs_the_interpreter_with_an_honest_report() {
    let (mut vm, module) = setup(compile("fn main() -> i32 { 40 + 2 }", false), None);
    let preparation = PreparedNativeEntry::Unsupported {
        backend: BackendId::new("fixture"),
        diagnostics: vec!["unsupported operation".into()],
    };
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    assert_eq!(report.return_value, Value::I32(42));
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics, ["unsupported operation"]);
}

#[test]
fn native_errors_never_restart_in_the_interpreter_and_keep_the_trace() {
    let (mut vm, module) = setup(compile("fn main() {}", false), None);
    let preparation = prepared(&vm, &module, native_trap);
    let error = vm
        .execute_prepared(&module, "main", &preparation)
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ScriptTrap)
    );
    assert_eq!(error.trace().unwrap().frames[0].instruction_offset, 0);
    // An interpreter restart would add the function's return charge and succeed.
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 1);
    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn policy_changes_fall_back_before_native_entry() {
    let (mut vm, module) = setup(compile("fn main() {}", false), None);
    let preparation = prepared(&vm, &module, native_trap);
    vm.runtime_mut().set_security_context(Default::default());
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 2);
}

#[test]
fn prepared_function_identity_must_match_the_requested_entry() {
    let bytecode = compile("fn main() {} fn other() {}", false);
    let (mut vm, module) = setup(bytecode, None);
    let preparation = prepared(&vm, &module, native_unit);
    let error = vm
        .execute_prepared(&module, "other", &preparation)
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 0);
}

#[test]
fn optimized_execution_preserves_results_traps_and_every_budget_failure_point() {
    for source in [
        "fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }",
        "fn main() -> i8 { 127i8 + 1i8 }",
        "fn main() -> u8 { 255u8 << 1 }",
        "fn main() -> i32 { 1 / 0 }",
        "fn main() -> i32 { var x = 0; while x < 3 { x += 1; } x }",
    ] {
        let original = compile(source, false);
        let optimized = compile(source, true);
        for budget in 0..40 {
            let (mut before, a) = setup(original.clone(), Some(budget));
            let (mut after, b) = setup(optimized.clone(), Some(budget));
            let a = before.execute(&a, "main");
            let b = after.execute(&b, "main");
            match (a, b) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a.return_value, b.return_value, "{source} budget={budget}")
                }
                (Err(a), Err(b)) => {
                    let describe = |error: &VmError| match error.cause() {
                        VmError::RuntimeError(error) => (error.kind(), error.message().to_owned()),
                        VmError::BuiltinError(error) => (error.kind(), error.message().to_owned()),
                        error => panic!("unexpected error: {error:?}"),
                    };
                    assert_eq!(describe(&a), describe(&b), "{source} budget={budget}");
                    let locations = |error: &VmError| {
                        error
                            .trace()
                            .unwrap()
                            .frames
                            .iter()
                            .map(|frame| (frame.instruction_offset, frame.source_span))
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(locations(&a), locations(&b), "{source} budget={budget}");
                }
                (a, b) => panic!("execution differs for {source} budget={budget}: {a:?}, {b:?}"),
            }
            assert_eq!(
                before.runtime().resources().counters(),
                after.runtime().resources().counters()
            );
            assert_eq!(after.runtime().gc().active_roots(), 0);
        }
    }
}

#[test]
fn debugging_selects_interpreter_before_entering_native_code() {
    use kagari_runtime::DebugVisibilityPolicy;
    use kagari_vm::DebugSession;
    let (mut vm, module) = setup(compile("fn main() {}", false), None);
    let preparation = prepared(&vm, &module, native_trap);
    let mut security = vm.runtime().security();
    security.profile.allow_debugger = true;
    security.capabilities.debug_attach = true;
    vm.runtime_mut().set_security_context(security);
    vm.runtime_mut()
        .set_debug_visibility_policy(DebugVisibilityPolicy {
            allow_all_modules: true,
            ..Default::default()
        });
    vm.attach_debug_session(DebugSession::new(vm.runtime()).unwrap())
        .unwrap();
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert!(jit.diagnostics[0].contains("observer"));
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 2);
}

#[test]
fn preparation_from_an_old_version_cannot_execute_as_a_new_version() {
    let (mut vm, module) = setup(compile("fn main() {}", false), None);
    let preparation = prepared(&vm, &module, native_unit);
    let new = vm
        .reload_program(
            &module,
            "test",
            BytecodeProgram {
                root: ModuleRef::new(0),
                modules: vec![(*module.bytecode).clone()],
            },
        )
        .unwrap();
    let error = vm.execute_prepared(&new, "main", &preparation).unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
    assert_eq!(vm.runtime().resources().counters().instruction_steps, 0);
    assert_eq!(
        vm.execute_prepared(&module, "main", &preparation)
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::Native
    );
}
