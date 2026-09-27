use std::ffi::c_void;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use kagari_abi::ids::FunctionRef;
use kagari_abi::native::{
    BackendId, BackendTarget, ExecutableEntryPoint, ExecutableFunctionArtifact, NativeCodeOwner,
    NativeCompilationProduct,
};
use kagari_abi::native_call::{JIT_STATUS_OK, JitValue};
use kagari_codegen::{
    BackendCompileError, BackendConfiguration, BackendDiagnostic, BackendDiagnosticKind,
    BackendFunctionInput, CodegenBackend,
};
use kagari_common::{SourceFile, cancellation::CancellationToken};
use kagari_embed::program::{NativePreparationError, PreparedProgram, ProgramPreparationError};
use kagari_embed::{ArtifactOptions, ExecutionContext, KagariEngine, NativeInputExport};
use kagari_mir::{Constant, Instruction, Terminator};
use kagari_runtime::{CapabilitySet, LanguageProfile, jit_abi::jit_consume_instruction_step};
use kagari_vm::{JitExecutionStatus, PreparedNativeEntry};

#[derive(Debug)]
struct Owner(Arc<AtomicUsize>);
impl NativeCodeOwner for Owner {}
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
unsafe extern "C" fn unit(runtime: *const c_void, result: *mut JitValue) -> i32 {
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
#[derive(Default)]
struct Backend {
    calls: usize,
    option: String,
    mode: u8,
    dropped: Arc<AtomicUsize>,
    cancel_during_compile: Option<CancellationToken>,
}
// SAFETY: the success path accepts exactly the two-point unit function and returns
// a process-lifetime ABI fixture implementing its charges and result. Other modes
// return errors and never executable products.
unsafe impl CodegenBackend for Backend {
    fn configuration(&self) -> BackendConfiguration {
        BackendConfiguration {
            backend: BackendId::new("unit-fixture"),
            target: BackendTarget::new("host-fixture", usize::BITS as u8),
            options: vec![
                ("fixture-mode".into(), self.mode.to_string()),
                ("fixture-option".into(), self.option.clone()),
            ],
        }
    }
    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeCompilationProduct, BackendCompileError> {
        self.calls += 1;
        if let Some(cancel) = &self.cancel_during_compile {
            cancel.cancel();
        }
        if self.mode == 1 {
            return Err(BackendCompileError::unsupported("fixture unsupported"));
        }
        if self.mode == 2 {
            return Err(BackendCompileError {
                diagnostics: vec![BackendDiagnostic {
                    kind: BackendDiagnosticKind::InternalError,
                    message: "fixture compiler failure".into(),
                }],
            });
        }
        let function = input.function();
        assert_eq!(function.blocks.len(), 1);
        let block = &function.blocks[0];
        let [
            Instruction::LoadConst {
                dst,
                constant: Constant::Unit,
            },
        ] = block.instructions.as_slice()
        else {
            panic!("unit fixture input");
        };
        assert!(matches!(block.terminator, Some(Terminator::Return(Some(value))) if value == *dst));
        assert_eq!(input.links().helpers.len(), 1);
        assert_eq!(
            input.links().helpers[0].address,
            jit_consume_instruction_step as *const () as usize
        );
        let configuration = self.configuration();
        let mut artifact = ExecutableFunctionArtifact::new(
            configuration.backend,
            configuration.target,
            FunctionRef::new(input.function_ref().index()),
        );
        artifact.entry = ExecutableEntryPoint::Native {
            symbol: "unit".into(),
            address: unit as *const () as usize,
        };
        Ok(NativeCompilationProduct {
            artifact,
            owner: Arc::new(Owner(self.dropped.clone())),
        })
    }
}
fn context() -> ExecutionContext {
    ExecutionContext {
        language_profile: LanguageProfile {
            allow_jit: true,
            ..Default::default()
        },
        capabilities: CapabilitySet {
            jit: true,
            ..Default::default()
        },
        ..Default::default()
    }
}
fn prepare(engine: &KagariEngine, native: NativeInputExport) -> PreparedProgram {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("shared", "fn main() {}"),
            Default::default(),
            ArtifactOptions {
                native_input: native,
                ..Default::default()
            },
        )
        .unwrap();
    PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}
#[test]
fn caches_products_across_runtimes_and_retains_them_after_backend_drop() {
    let engine = KagariEngine::default();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let copy = program.clone();
    let context = context();
    let mut a = engine.runtime(context.clone());
    let mut b = engine.runtime(context.clone());
    let ma = a.load_program(&program, Default::default()).unwrap();
    let mb = b.load_program(&copy, Default::default()).unwrap();
    let mut backend = Backend::default();
    let dropped = backend.dropped.clone();
    let pa = a
        .prepare_native(&program, &ma, "main", &mut backend, &Default::default())
        .unwrap();
    let pb = b
        .prepare_native(&copy, &mb, "main", &mut backend, &Default::default())
        .unwrap();
    assert_eq!(backend.calls, 1);
    backend.option = "alternate".into();
    let alternate = a
        .prepare_native(&program, &ma, "main", &mut backend, &Default::default())
        .unwrap();
    assert_eq!(backend.calls, 2);
    drop(backend);
    drop(program);
    drop(copy);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    for (runtime, module, prepared) in [(&mut a, &ma, &pa), (&mut b, &mb, &pb)] {
        let report = runtime
            .execute_prepared(module, "main", &[], &context, prepared)
            .unwrap();
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    }
    assert!(b.execute_prepared(&mb, "main", &[], &context, &pa).is_err());
    drop(pa);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(pb);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    drop(alternate);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}
#[test]
fn rejects_equal_but_distinct_versions_before_compiling() {
    let engine = KagariEngine::default();
    let first = prepare(&engine, NativeInputExport::PortableMir);
    let second = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime.load_program(&first, Default::default()).unwrap();
    let mut backend = Backend::default();
    assert!(matches!(
        runtime.prepare_native(&second, &loaded, "main", &mut backend, &Default::default()),
        Err(NativePreparationError::WrongVersion)
    ));
    assert_eq!(backend.calls, 0);
}
#[test]
fn missing_input_and_unsupported_functions_fall_back_but_compiler_errors_do_not() {
    let engine = KagariEngine::default();
    let context = context();
    let mut runtime = engine.runtime(context.clone());
    let bytecode = prepare(&engine, NativeInputExport::BytecodeOnly);
    let loaded = runtime.load_program(&bytecode, Default::default()).unwrap();
    let mut backend = Backend::default();
    let missing = runtime
        .prepare_native(
            &bytecode,
            &loaded,
            "main",
            &mut backend,
            &Default::default(),
        )
        .unwrap();
    assert!(matches!(missing, PreparedNativeEntry::Unsupported { .. }));
    assert_eq!(backend.calls, 0);
    assert_eq!(
        runtime
            .execute_prepared(&loaded, "main", &[], &context, &missing)
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::InterpreterFallback
    );
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    backend.mode = 1;
    for _ in 0..2 {
        assert!(matches!(
            runtime
                .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
                .unwrap(),
            PreparedNativeEntry::Unsupported { .. }
        ));
    }
    assert_eq!(backend.calls, 1);
    backend.mode = 2;
    for _ in 0..2 {
        assert!(matches!(
            runtime.prepare_native(&program, &loaded, "main", &mut backend, &Default::default()),
            Err(NativePreparationError::Compile(_))
        ));
    }
    assert_eq!(backend.calls, 3);
}
#[test]
fn cancellation_discards_compilation_results_and_can_be_retried() {
    let engine = KagariEngine::default();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let cancel = CancellationToken::default();
    let mut backend = Backend {
        cancel_during_compile: Some(cancel.clone()),
        ..Default::default()
    };
    assert!(matches!(
        runtime.prepare_native(&program, &loaded, "main", &mut backend, &cancel),
        Err(NativePreparationError::Cancelled)
    ));
    assert_eq!(backend.dropped.load(Ordering::SeqCst), 1);
    backend.cancel_during_compile = None;
    runtime
        .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
        .unwrap();
    assert_eq!(backend.calls, 2);
}
#[test]
fn mismatched_artifact_native_input_is_rejected_before_loading() {
    let engine = KagariEngine::default();
    let mut artifact = engine
        .compile_to_artifact(
            SourceFile::new("a", "fn main() {}"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let other = engine
        .compile_to_artifact(
            SourceFile::new("a", "fn main() -> i32 { 1 }"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    // Construct a valid envelope with independently supplied compiler input.
    artifact = kagari_bytecode::KbcArtifact::from_program(
        artifact.program,
        kagari_bytecode::ArtifactBuildOptions {
            portable_mir: other.portable_mir,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()),
        Err(ProgramPreparationError::NativeInput(_))
    ));
}

#[test]
fn bounded_cache_keeps_existing_entries_usable_after_capacity_is_reached() {
    let engine = KagariEngine::default();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let mut backend = Backend {
        mode: 1,
        ..Default::default()
    };
    for index in 0..4096 {
        backend.option = index.to_string();
        runtime
            .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
            .unwrap();
    }
    backend.option = "overflow".into();
    assert!(matches!(
        runtime.prepare_native(&program, &loaded, "main", &mut backend, &Default::default()),
        Err(NativePreparationError::CacheLimit)
    ));
    assert_eq!(backend.calls, 4096);
    backend.option = "0".into();
    runtime
        .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
        .unwrap();
    assert_eq!(backend.calls, 4096);
}

#[test]
fn policy_rejection_and_pre_cancelled_requests_do_not_compile() {
    let engine = KagariEngine::default();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let mut backend = Backend::default();
    assert!(matches!(
        runtime
            .prepare_native(&program, &loaded, "main", &mut backend, &Default::default())
            .unwrap(),
        PreparedNativeEntry::Unsupported { .. }
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        runtime.prepare_native(&program, &loaded, "main", &mut backend, &cancel),
        Err(NativePreparationError::Cancelled)
    ));
    assert_eq!(backend.calls, 0);
}
