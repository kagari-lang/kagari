#[cfg(test)]
use crate::tests::support::bytecode::load_bytecode;
use crate::{catalog as foundation_catalog, tests::support::bytecode::reload_bytecode};
use kagari_runtime::module::retention::RetentionOrigin;
use kagari_runtime::{
    Runtime, RuntimeConfig,
    debug::DebugSession,
    error::{RuntimeError, RuntimeErrorKind},
    execution::{JitExecutionStatus, native::PreparedNativeEntry},
    jit_abi::jit_poll_execution,
    module::{ExecutableModule, InstalledProgram, entry::ProgramEntry},
    native::conversion::context::ConversionContext,
    native::module::NativeModule,
    resource::RuntimeLimits,
};

use kagari_abi::{
    native::{BackendId, BackendTarget, ExecutableEntryPoint, NativeCodeOwner},
    native_call::{
        JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK, JitCompiledFunction, JitValue,
        NativeInvocationContext,
    },
};
use kagari_bytecode::program::BytecodeProgram;
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_compiler::{
    bytecode::lower_program_to_bytecode,
    source::{lower::instances::MirLoweringOptions, program::lower_program_to_mir},
};
use kagari_contract::{
    ids::FunctionRef,
    native::{ExecutableFunctionArtifact, NativeCompilationProduct},
};
use kagari_hir::{analysis::AnalysisDatabase, compile_input::program::CheckedCompilation};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

use rstest::rstest;
use std::sync::Arc;

fn compile(source: &str, optimize: bool) -> DefinitionMetadata<BytecodeProgram> {
    let mut sources = SourceDatabase::default();
    let root = sources
        .set("test.kgr", source.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analysis_database()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let checked = snapshot.check_program(root, &Default::default()).unwrap();
    let mir = lower_program_to_mir(
        &CheckedCompilation::from_program(&checked, &Default::default()).unwrap(),
        &MirLoweringOptions {
            optimization: optimize.then(Default::default),
            ..Default::default()
        },
    )
    .unwrap();
    lower_program_to_bytecode(&mir, &Default::default())
        .map(|program| program.into_unverified())
        .unwrap()
}

fn setup(bytecode: DefinitionMetadata<BytecodeProgram>) -> (Runtime, InstalledProgram) {
    let mut runtime = Runtime::new(RuntimeConfig {
        limits: RuntimeLimits {
            ..Default::default()
        },
        ..Default::default()
    });
    NativeModule::install_all(&crate::modules().unwrap(), &mut runtime).unwrap();
    let loaded = load_bytecode(&mut runtime, "test", bytecode).unwrap();
    (runtime, loaded)
}

#[derive(Debug)]
struct StaticCode;

impl NativeCodeOwner for StaticCode {}

unsafe extern "C" fn native_unit(
    runtime: *const NativeInvocationContext,
    result: *mut JitValue,
) -> i32 {
    for offset in 0..2 {
        let status = unsafe { jit_poll_execution((*runtime).runtime.cast_mut().cast(), offset) };
        if status != JIT_STATUS_OK {
            return status;
        }
    }
    unsafe {
        result.write(JitValue::unit());
    }
    JIT_STATUS_OK
}

unsafe extern "C" fn native_trap(runtime: *const NativeInvocationContext, _: *mut JitValue) -> i32 {
    let status = unsafe { jit_poll_execution((*runtime).runtime.cast_mut().cast(), 0) };
    if status == JIT_STATUS_OK {
        JIT_STATUS_INTEGER_OVERFLOW
    } else {
        status
    }
}

fn prepared(
    vm: &mut Runtime,
    module: &ExecutableModule,
    entry: JitCompiledFunction,
) -> PreparedNativeEntry {
    let mut artifact = ExecutableFunctionArtifact::new(
        BackendId::new("fixture"),
        BackendTarget::new("host", usize::BITS as u8),
        FunctionRef::new(0),
    );
    artifact.code.entry = ExecutableEntryPoint::Native {
        symbol: "fixture".into(),
        address: entry as usize,
    };
    let product = Arc::new(NativeCompilationProduct {
        artifact,
        owner: Arc::new(StaticCode),
    });
    // Static ABI fixture; trap probe intentionally exercises failure propagation.
    PreparedNativeEntry::Native(
        unsafe { vm.install_native_function(module, product, RetentionOrigin::Current) }.unwrap(),
    )
}

#[test]
fn prepared_native_execution_reports_the_installed_descriptor_and_cleans_frames() {
    let (mut vm, module) = setup(compile(
        include_str!("../../../tests/fixtures/execution/prepared-unit.kgr"),
        false,
    ));
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    assert_eq!(module.bytecode.functions[0].instructions.len(), 2);
    let preparation = prepared(&mut vm, &module, native_unit);
    let report = vm
        .execute_prepared_module(
            &module,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap();
    ConversionContext::new(&mut vm, &module)
        .unwrap()
        .decode::<()>(&report.return_value)
        .unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert_eq!(jit.artifact.unwrap().function, FunctionRef::new(0));
    assert!(jit.diagnostics.is_empty());

    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn unsupported_preparation_runs_the_interpreter_with_an_honest_report() {
    let (mut vm, module) = setup(compile(
        include_str!("../../../tests/fixtures/execution/prepared-fallback.kgr"),
        false,
    ));
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    let preparation = PreparedNativeEntry::Unsupported {
        backend: BackendId::new("fixture"),
        diagnostics: vec!["unsupported operation".into()],
    };
    let report = vm
        .execute_prepared_module(
            &module,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap();
    assert_eq!(report.return_value.as_i32(), Some(42));
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(jit.diagnostics, ["unsupported operation"]);
}

#[test]
fn native_errors_never_restart_in_the_interpreter_and_keep_the_trace() {
    let (mut vm, module) = setup(compile(
        include_str!("../../../tests/fixtures/execution/prepared-unit.kgr"),
        false,
    ));
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    let preparation = prepared(&mut vm, &module, native_trap);
    let error = vm
        .execute_prepared_module(
            &module,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap_err();
    assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::ScriptTrap));
    assert_eq!(error.trace().unwrap().frames[0].instruction_offset, 0);
    // An interpreter restart would add the function's return charge and succeed.

    assert_eq!(vm.resources().counters().current_call_depth, 0);
}

#[test]
fn prepared_function_identity_must_match_the_requested_entry() {
    let bytecode = compile(
        include_str!("../../../tests/fixtures/execution/prepared-other.kgr"),
        false,
    );
    let (mut vm, module) = setup(bytecode);
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    let preparation = prepared(&mut vm, &module, native_unit);
    let error = vm
        .execute_prepared_module(&module, "other", &preparation, vm.execution_options())
        .unwrap_err();
    assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::ModuleValidation));
}

#[rstest]
#[case::folded_branch(include_str!("../../../tests/fixtures/execution/prepared-folded-branch.kgr"))]
#[case::signed_overflow(include_str!("../../../tests/fixtures/execution/prepared-signed-overflow.kgr"))]
#[case::unsigned_shift(include_str!("../../../tests/fixtures/execution/prepared-unsigned-shift.kgr"))]
#[case::division_zero(include_str!("../../../tests/fixtures/execution/prepared-division-zero.kgr"))]
#[case::while_loop(include_str!("../../../tests/fixtures/execution/prepared-while-loop.kgr"))]
#[case::snapshot_before_write(include_str!("../../../tests/fixtures/execution/prepared-snapshot-before-write.kgr"))]
#[case::remainder_loop(
    include_str!("../../../tests/fixtures/execution/prepared-remainder-loop.kgr")
)]
#[case::argument_snapshots(
    include_str!("../../../tests/fixtures/execution/prepared-argument-snapshots.kgr")
)]
fn optimized_execution_preserves_results_traps_and_source_origins(#[case] source: &str) {
    let original = compile(source, false);
    let optimized = compile(source, true);
    {
        let (mut before, a) = setup(original.clone());
        let _a_program = a;
        let a = before.select_program(&_a_program).unwrap();
        let (mut after, b) = setup(optimized.clone());
        let _b_program = b;
        let b = after.select_program(&_b_program).unwrap();
        before.collect_garbage().unwrap();
        after.collect_garbage().unwrap();
        let roots = after.gc_stats().root_registrations;
        let a = before.execute_module(&a, ProgramEntry::Main, before.execution_options());
        let b = after.execute_module(&b, ProgramEntry::Main, after.execution_options());
        match (a, b) {
            (Ok(a), Ok(b)) => {
                let left = (a.return_value.as_i32(), a.return_value.as_u64());
                let right = (b.return_value.as_i32(), b.return_value.as_u64());
                assert!(
                    left.0.is_some() || left.1.is_some(),
                    "scalar optimization fixture"
                );
                assert_eq!(left, right, "{source}");
            }
            (Err(a), Err(b)) => {
                let describe = |error: &RuntimeError| (error.kind(), error.to_string());
                assert_eq!(describe(&a), describe(&b), "{source}");
                let locations = |error: &RuntimeError| {
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
        assert_eq!(before.resources().counters(), after.resources().counters());
        assert_eq!(after.gc_stats().root_registrations, roots);
    }
}

#[test]
fn debugging_selects_interpreter_before_entering_native_code() {
    let (mut vm, module) = setup(compile(
        include_str!("../../../tests/fixtures/execution/prepared-unit.kgr"),
        false,
    ));
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    let preparation = prepared(&mut vm, &module, native_trap);

    let debug = DebugSession::new(&vm).unwrap();
    vm.attach_debug_session(debug).unwrap();
    let report = vm
        .execute_prepared_module(
            &module,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert!(jit.diagnostics[0].contains("observer"));
}

#[test]
fn preparation_from_an_old_version_cannot_execute_as_a_new_version() {
    let program = compile(
        include_str!("../../../tests/fixtures/execution/prepared-unit.kgr"),
        false,
    );
    let (mut vm, module) = setup(program.clone());
    let _module_program = module;
    let module = vm.select_program(&_module_program).unwrap();
    let preparation = prepared(&mut vm, &module, native_unit);
    reload_bytecode(&mut vm, &_module_program, Some("test".into()), program).unwrap();
    let _new_program = _module_program.clone();
    let new = vm.select_program(&_new_program).unwrap();
    let error = vm
        .execute_prepared_module(
            &new,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap_err();
    assert!(matches!(&error, error if error.kind() == RuntimeErrorKind::ModuleValidation));

    let error = vm
        .execute_prepared_module(
            &module,
            ProgramEntry::Main,
            &preparation,
            vm.execution_options(),
        )
        .unwrap_err();
    assert!(matches!(&error, error
        if error.kind() == RuntimeErrorKind::ModuleValidation));
    let current_preparation = prepared(&mut vm, &new, native_unit);
    assert_eq!(
        vm.execute_prepared_module(
            &new,
            ProgramEntry::Main,
            &current_preparation,
            vm.execution_options()
        )
        .unwrap()
        .jit
        .unwrap()
        .status,
        JitExecutionStatus::Native
    );
}

fn analysis_database() -> AnalysisDatabase {
    let mut database = AnalysisDatabase::default();
    database.set_native_modules(foundation_catalog::shared().unwrap());
    database
}
