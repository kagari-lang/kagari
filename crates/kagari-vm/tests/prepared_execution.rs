use std::{ffi::c_void, rc::Rc};

use kagari_abi::{
    ids::FunctionRef,
    native::{
        BackendId, BackendTarget, ExecutableEntryPoint, ExecutableFunctionArtifact,
        NativeCodeOwner, NativeCompilationProduct,
    },
    native_call::{JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK, JitCompiledFunction, JitValue},
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::source_database::{SourceDatabase, SourceLayer};
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_hir::analysis::AnalysisDatabase;
use kagari_runtime::{
    Runtime, RuntimeConfig, error::RuntimeErrorKind, jit_abi::jit_poll_execution,
    module::LoadedModule, resource::RuntimeLimits, value::Value,
};
use kagari_vm::{
    error::VmError,
    vm::{JitExecutionStatus, Vm, native::PreparedNativeEntry},
};

fn compile(source: &str, optimize: bool) -> BytecodeProgram {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &checked,
        &MirLoweringOptions {
            optimization: optimize.then(Default::default),
            ..Default::default()
        },
    )
    .unwrap();
    lower_program_to_bytecode(&mir).unwrap()
}
fn setup(bytecode: BytecodeProgram) -> (Vm, LoadedModule) {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    let loaded = runtime.load_program("test", bytecode).unwrap();
    (Vm::new(runtime), loaded)
}
#[derive(Debug)]
struct StaticCode;
impl NativeCodeOwner for StaticCode {}
unsafe extern "C" fn native_unit(runtime: *const c_void, result: *mut JitValue) -> i32 {
    for offset in 0..2 {
        let status = unsafe { jit_poll_execution(runtime.cast(), offset) };
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
    let status = unsafe { jit_poll_execution(runtime.cast(), 0) };
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
    let (mut vm, module) = setup(compile("fn main() {}", false));
    assert_eq!(module.bytecode.functions[0].instructions.len(), 2);
    let preparation = prepared(&vm, &module, native_unit);
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    assert_eq!(report.return_value, Value::Unit);
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert_eq!(jit.artifact.unwrap().function, FunctionRef::new(0));
    assert!(jit.diagnostics.is_empty());

    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn unsupported_preparation_runs_the_interpreter_with_an_honest_report() {
    let (mut vm, module) = setup(compile("fn main() -> i32 { 40 + 2 }", false));
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
    let (mut vm, module) = setup(compile("fn main() {}", false));
    let preparation = prepared(&vm, &module, native_trap);
    let error = vm
        .execute_prepared(&module, "main", &preparation)
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ScriptTrap)
    );
    assert_eq!(error.trace().unwrap().frames[0].instruction_offset, 0);
    // An interpreter restart would add the function's return charge and succeed.

    assert_eq!(vm.runtime().resources().counters().current_call_depth, 0);
}

#[test]
fn prepared_function_identity_must_match_the_requested_entry() {
    let bytecode = compile("fn main() {} fn other() {}", false);
    let (mut vm, module) = setup(bytecode);
    let preparation = prepared(&vm, &module, native_unit);
    let error = vm
        .execute_prepared(&module, "other", &preparation)
        .unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );
}

#[test]
fn optimized_execution_preserves_results_traps_and_source_origins() {
    for source in [
        "fn main() -> i32 { if 1 + 2 == 3 { 42 } else { 0 } }",
        "fn main() -> i8 { 127i8 + 1i8 }",
        "fn main() -> u8 { 255u8 << 1 }",
        "fn main() -> i32 { 1 / 0 }",
        "fn main() -> i32 { var x = 0; while x < 3 { x += 1; } x }",
    ] {
        let original = compile(source, false);
        let optimized = compile(source, true);
        {
            let (mut before, a) = setup(original.clone());
            let (mut after, b) = setup(optimized.clone());
            let a = before.execute(&a, "main");
            let b = after.execute(&b, "main");
            match (a, b) {
                (Ok(a), Ok(b)) => {
                    assert_eq!(a.return_value, b.return_value, "{source}")
                }
                (Err(a), Err(b)) => {
                    let describe = |error: &VmError| match error.cause() {
                        VmError::RuntimeError(error) => (error.kind(), error.message().to_owned()),
                        VmError::BuiltinError(error) => (error.kind(), error.message().to_owned()),
                        error => panic!("unexpected error: {error:?}"),
                    };
                    assert_eq!(describe(&a), describe(&b), "{source}");
                    let locations = |error: &VmError| {
                        error
                            .trace()
                            .unwrap()
                            .frames
                            .iter()
                            .map(|frame| frame.source_span)
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(locations(&a), locations(&b), "{source}");
                }
                (a, b) => panic!("execution differs for {source}: {a:?}, {b:?}"),
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
    use kagari_vm::debug::DebugSession;
    let (mut vm, module) = setup(compile("fn main() {}", false));
    let preparation = prepared(&vm, &module, native_trap);

    vm.attach_debug_session(DebugSession::new(vm.runtime()).unwrap())
        .unwrap();
    let report = vm.execute_prepared(&module, "main", &preparation).unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert!(jit.diagnostics[0].contains("observer"));
}

#[test]
fn preparation_from_an_old_version_cannot_execute_as_a_new_version() {
    let program = compile("fn main() {}", false);
    let (mut vm, module) = setup(program.clone());
    let preparation = prepared(&vm, &module, native_unit);
    let new = vm.reload_program(&module, "test", program).unwrap();
    let error = vm.execute_prepared(&new, "main", &preparation).unwrap_err();
    assert!(
        matches!(error.cause(), VmError::RuntimeError(error) if error.kind() == RuntimeErrorKind::ModuleValidation)
    );

    assert_eq!(
        vm.execute_prepared(&module, "main", &preparation)
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::Native
    );
}
