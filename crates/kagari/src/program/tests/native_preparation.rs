use crate::program::native::NativeCompiler;
use kagari_bytecode::artifact::{ArtifactBuildOptions, KbcArtifact};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
#[cfg(feature = "native")]
use kagari_runtime::error::RuntimeErrorKind;
use kagari_runtime::reload::ReloadValidationError;
use kagari_runtime::{
    Runtime,
    error::RuntimeError,
    frame::ExecutionFrame,
    module::entry::ProgramEntry,
    session::{ExecutionEvent, ExecutionObserver},
    value::Value,
};
use kagari_stdlib as stdlib;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use crate::{
    context::{ExecutionContext, JitPolicy},
    engine::{
        KagariEngine,
        source::{ArtifactOptions, NativeInputExport},
    },
    program::{
        Compilation, CompilationPreparationError,
        native::{NativeCode, NativePreparationError},
    },
};
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput, CodegenBackend,
    diagnostic::{BackendCompileError, BackendDiagnostic, BackendDiagnosticKind},
};
use kagari_mir::instruction::{Constant, Instruction, Terminator};
use kagari_runtime::execution::{JitExecutionStatus, native::PreparedNativeEntry};
use {
    kagari_abi::{
        native::{BackendId, BackendTarget, ExecutableEntryPoint, NativeCodeOwner},
        native_call::{
            JIT_STATUS_INTEGER_OVERFLOW, JIT_STATUS_OK, JitValue, NativeInvocationContext,
        },
    },
    kagari_contract::{
        ids::FunctionRef,
        native::{ExecutableFunctionArtifact, NativeCompilationProduct},
    },
};
use {kagari_common::cancellation::CancellationToken, kagari_source::source::SourceFile};

#[derive(Debug)]
struct Owner(Arc<AtomicUsize>);

impl NativeCodeOwner for Owner {}

impl Drop for Owner {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

unsafe extern "C" fn unit(runtime: *const NativeInvocationContext, result: *mut JitValue) -> i32 {
    for offset in 0..2 {
        let status = unsafe { ((*(*runtime).helpers).poll_execution)((*runtime).runtime, offset) };
        if status != JIT_STATUS_OK {
            return status;
        }
    }
    unsafe {
        result.write(JitValue::unit());
    }
    JIT_STATUS_OK
}

unsafe extern "C" fn trap(runtime: *const NativeInvocationContext, _: *mut JitValue) -> i32 {
    let status = unsafe { ((*(*runtime).helpers).poll_execution)((*runtime).runtime, 0) };
    if status == JIT_STATUS_OK {
        JIT_STATUS_INTEGER_OVERFLOW
    } else {
        status
    }
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
// a process-lifetime ABI fixture implementing its polls and result. Mode 3 uses
// the same input with a deliberate trap to detect native entry and interpreter
// restarts. Other modes return errors and never executable products.
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
        let configuration = self.configuration();
        let mut artifact = ExecutableFunctionArtifact::new(
            configuration.backend,
            configuration.target,
            FunctionRef::new(input.function_ref().index()),
        );
        artifact.code.entry = ExecutableEntryPoint::Native {
            symbol: "unit".into(),
            address: if self.mode == 3 {
                trap as *const () as usize
            } else {
                unit as *const () as usize
            },
        };
        Ok(NativeCompilationProduct {
            artifact,
            owner: Arc::new(Owner(self.dropped.clone())),
        })
    }
}

fn context() -> ExecutionContext {
    ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    }
}

fn prepare(engine: &KagariEngine, native: NativeInputExport) -> Compilation {
    prepare_source(engine, native, "fn main() {}")
}

fn prepare_source(engine: &KagariEngine, native: NativeInputExport, source: &str) -> Compilation {
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("shared", source),
            ArtifactOptions {
                native_input: native,
                ..Default::default()
            },
        )
        .unwrap();
    Compilation::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
}

#[test]
fn disabled_installation_validates_before_choosing_local_fallback() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    let mut backend = Backend::default();
    let prepared = compiler
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        .unwrap();
    let PreparedNativeEntry::Unsupported { diagnostics, .. } = &prepared else {
        panic!("disabled installation must not admit native code");
    };
    assert_eq!(diagnostics, &["JIT disabled by execution context policy"]);
    // Enabling execution cannot compile or replace a prior preparation decision.
    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &context(), &prepared)
        .unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert_eq!(&jit.diagnostics, diagnostics);
    drop(report.return_value);

    let other = prepare(&engine, NativeInputExport::PortableMir);
    assert!(matches!(
        {
            let native_code = compiler.prepare(
                &other,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            );
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::WrongVersion)
    ));
    assert!(matches!(
        {
            let native_code =
                compiler.prepare(&program, "missing", &mut backend, &Default::default());
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::MissingEntry(_))
    ));
    let mut foreign = engine.runtime(context());
    let foreign_module = foreign
        .load_program(program.program(), Default::default())
        .unwrap();
    assert!(matches!(
        {
            let native_code = compiler.prepare(
                &program,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            );
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &foreign_module))
        },
        Err(NativePreparationError::Runtime(_))
    ));
    // Explicit detached compilation is independent of each recipient's policy.
    assert_eq!(backend.calls, 2);
    assert_eq!(runtime.gc().active_roots(), 0);
}

#[derive(Debug)]
struct Observer(Arc<AtomicUsize>);

impl ExecutionObserver for Observer {
    fn observe(
        &mut self,
        _: &Runtime,
        _: ExecutionEvent,
        _: &[ExecutionFrame],
    ) -> Result<(), RuntimeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn execution_policy_preserves_cancellation_observers_and_native_failures() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let enabled = context();
    let mut runtime = engine.runtime(enabled.clone());
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    let _selected_loaded = runtime.select_program(&loaded).unwrap();
    let mut backend = Backend {
        mode: 3,
        ..Default::default()
    };
    let prepared = compiler
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        .unwrap();
    assert!(matches!(prepared, PreparedNativeEntry::Native(_)));
    // The fixture traps on native entry; the interpreter body simply returns Unit.
    let report = runtime
        .execute_prepared(
            &loaded,
            ProgramEntry::Main,
            &[],
            &Default::default(),
            &prepared,
        )
        .unwrap();
    assert_eq!(report.return_value.value(runtime.gc()), Some(Value::Unit));
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert_eq!(
        jit.diagnostics,
        ["JIT disabled by execution context policy"]
    );
    assert!(jit.artifact.is_none());
    drop(report.return_value);
    let error = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &enabled, &prepared)
        .unwrap_err();
    assert_eq!(error.trace().unwrap().frames[0].instruction_offset, 0);
    assert!(matches!(
        error,
        ref error if error.kind() == RuntimeErrorKind::ScriptTrap
    ));

    for jit_policy in [JitPolicy::Enabled, JitPolicy::Disabled] {
        let cancelled = ExecutionContext {
            jit_policy,
            ..Default::default()
        };
        cancelled.cancellation.cancel();
        let error = runtime
            .execute_prepared(&loaded, ProgramEntry::Main, &[], &cancelled, &prepared)
            .unwrap_err();
        assert!(matches!(
            error,
            ref error if error.kind() == RuntimeErrorKind::Cancelled
        ));
    }
    let observed = Arc::new(AtomicUsize::new(0));
    runtime
        .set_execution_observer(Observer(observed.clone()))
        .unwrap();
    let session = runtime
        .begin_execution(&_selected_loaded, enabled.runtime_options())
        .unwrap();
    runtime.attach_execution_observer().unwrap();
    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &enabled, &prepared)
        .unwrap();
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
    assert!(jit.artifact.is_none());
    assert!(
        jit.diagnostics
            .iter()
            .any(|reason| reason.contains("observer"))
    );
    assert!(observed.load(Ordering::SeqCst) > 0);
    drop(report.return_value);
    drop(session);
    assert_eq!(runtime.gc().active_roots(), 0);
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
}

#[test]
fn caches_products_across_runtimes_and_retains_them_after_backend_drop() {
    fn send_sync<T: Send + Sync>() {}

    send_sync::<NativeCode>();
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare_source(
        &engine,
        NativeInputExport::PortableMir,
        "mod implementation { pub fn unit() {} } pub use crate::implementation::unit as run; pub use crate as again; fn main() {}",
    );
    let copy = program.clone();
    let context = context();
    let mut a = engine.runtime(context.clone());
    let mut b = engine.runtime(context.clone());
    let ma = a
        .load_program(program.program(), Default::default())
        .unwrap();
    let mb = b.load_program(copy.program(), Default::default()).unwrap();
    let mut backend = Backend::default();
    let dropped = backend.dropped.clone();
    let pa = compiler
        .prepare(&program, "again::run", &mut backend, &Default::default())
        .and_then(|native_code| (native_code).install(&mut a, &ma))
        .unwrap();
    let pb = compiler
        .prepare(&copy, "again::run", &mut backend, &Default::default())
        .and_then(|native_code| (native_code).install(&mut b, &mb))
        .unwrap();
    assert_eq!(backend.calls, 1);
    let PreparedNativeEntry::Native(installed) = &pa else {
        panic!("native fixture should compile");
    };
    assert_eq!(
        installed.module().bytecode.identity.path,
        ["implementation"]
    );
    assert!(matches!(
        compiler.prepare(&program, "main", &mut backend, &Default::default()).and_then(|native_code| (native_code).install(&mut a, &ma)),
        Err(NativePreparationError::MissingEntry(name)) if name == "main"
    ));
    assert_eq!(backend.calls, 1);
    for jit_policy in [JitPolicy::Enabled, JitPolicy::Disabled] {
        let execution = ExecutionContext {
            jit_policy,
            ..Default::default()
        };
        let mismatch = a
            .execute_prepared(&ma, ProgramEntry::Main, &[], &execution, &pa)
            .unwrap_err();
        assert!(
            matches!(mismatch, ref error if error.kind() == RuntimeErrorKind::ModuleValidation && error.to_string().contains("different function or execution version"))
        );
        assert!(
            b.execute_prepared(&mb, "again::run", &[], &execution, &pa)
                .is_err()
        );
    }
    backend.option = "alternate".into();
    let alternate = compiler
        .prepare(&program, "again::run", &mut backend, &Default::default())
        .and_then(|native_code| (native_code).install(&mut a, &ma))
        .unwrap();
    assert_eq!(backend.calls, 2);
    drop(compiler);
    drop(backend);
    drop(program);
    drop(copy);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    for (runtime, module, prepared) in [(&mut a, &ma, &pa), (&mut b, &mb, &pb)] {
        let report = runtime
            .execute_prepared(module, "again::run", &[], &context, prepared)
            .unwrap();
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    }
    drop(pa);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(pb);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    drop(alternate);
    assert_eq!(dropped.load(Ordering::SeqCst), 2);
}

#[test]
fn rejects_equal_but_distinct_versions_at_native_installation() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let first = prepare(&engine, NativeInputExport::PortableMir);
    let second = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime
        .load_program(first.program(), Default::default())
        .unwrap();
    let mut backend = Backend::default();
    assert!(matches!(
        {
            let native_code = compiler.prepare(
                &second,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            );
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::WrongVersion)
    ));
    assert_eq!(backend.calls, 1);
}

#[test]
fn missing_input_and_unsupported_functions_fall_back_but_compiler_errors_do_not() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let context = context();
    let mut runtime = engine.runtime(context.clone());
    let bytecode = prepare(&engine, NativeInputExport::BytecodeOnly);
    let loaded = runtime
        .load_program(bytecode.program(), Default::default())
        .unwrap();
    let mut backend = Backend::default();
    let missing = compiler
        .prepare(
            &bytecode,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        .unwrap();
    assert!(matches!(missing, PreparedNativeEntry::Unsupported { .. }));
    assert_eq!(backend.calls, 0);
    assert_eq!(
        runtime
            .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &missing)
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::InterpreterFallback
    );
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    backend.mode = 1;
    for _ in 0..2 {
        assert!(matches!(
            {
                let native_code = compiler.prepare(
                    &program,
                    ProgramEntry::Main,
                    &mut backend,
                    &Default::default(),
                );
                native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
            }
            .unwrap(),
            PreparedNativeEntry::Unsupported { .. }
        ));
    }
    assert_eq!(backend.calls, 1);
    backend.mode = 2;
    for _ in 0..2 {
        assert!(matches!(
            {
                let native_code = compiler.prepare(
                    &program,
                    ProgramEntry::Main,
                    &mut backend,
                    &Default::default(),
                );
                native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
            },
            Err(NativePreparationError::Compile(_))
        ));
    }
    assert_eq!(backend.calls, 3);
}

#[test]
fn cancellation_discards_compilation_results_and_can_be_retried() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    let cancel = CancellationToken::default();
    let mut backend = Backend {
        cancel_during_compile: Some(cancel.clone()),
        ..Default::default()
    };
    assert!(matches!(
        {
            let native_code = compiler.prepare(&program, ProgramEntry::Main, &mut backend, &cancel);
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::Cancelled)
    ));
    assert_eq!(backend.dropped.load(Ordering::SeqCst), 1);
    backend.cancel_during_compile = None;
    compiler
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        .unwrap();
    assert_eq!(backend.calls, 2);
}

#[test]
fn mismatched_artifact_native_input_is_rejected_before_loading() {
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("a", "fn main() {}"), Default::default())
        .unwrap();
    let other = engine
        .compile_to_artifact(
            SourceFile::new("a", "fn main() -> i32 { 1 }"),
            Default::default(),
        )
        .unwrap();
    // Construct a valid envelope with independently supplied compiler input.
    let definitions = DefinitionContext::new().unwrap();
    let artifact = artifact
        .import_into(&definitions, &Default::default())
        .unwrap()
        .into_records();
    let artifact = KbcArtifact::from_program(
        artifact.program,
        ArtifactBuildOptions {
            portable_mir: other.into_records().portable_mir,
            ..Default::default()
        },
        &definitions,
        &Default::default(),
    )
    .unwrap();
    let artifact =
        DefinitionMetadata::checked(definitions.snapshot(), artifact, &Default::default()).unwrap();
    assert!(matches!(
        Compilation::from_artifact(artifact, &Default::default(), &Default::default()),
        Err(CompilationPreparationError::NativeInput(_))
    ));
}

#[test]
fn bounded_cache_keeps_existing_entries_usable_after_capacity_is_reached() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(context());
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    let mut backend = Backend {
        mode: 1,
        ..Default::default()
    };
    for index in 0..4096 {
        backend.option = index.to_string();
        compiler
            .prepare(
                &program,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            )
            .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
            .unwrap();
    }
    backend.option = "overflow".into();
    assert!(matches!(
        {
            let native_code = compiler.prepare(
                &program,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            );
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::CacheLimit)
    ));
    assert_eq!(backend.calls, 4096);
    backend.option = "0".into();
    compiler
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        .unwrap();
    assert_eq!(backend.calls, 4096);
}

#[test]
fn pre_cancelled_requests_do_not_compile() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let program = prepare(&engine, NativeInputExport::PortableMir);
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(program.program(), Default::default())
        .unwrap();
    let mut backend = Backend::default();
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(matches!(
        {
            let native_code = compiler.prepare(&program, ProgramEntry::Main, &mut backend, &cancel);
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &loaded))
        },
        Err(NativePreparationError::Cancelled)
    ));
    assert_eq!(backend.calls, 0);
}

#[test]
fn reload_reuses_candidate_code_and_keeps_old_native_entries_version_pinned() {
    let mut compiler = NativeCompiler::default();
    let engine =
        KagariEngine::with_native_modules(Default::default(), stdlib::modules().unwrap()).unwrap();
    let old_program = prepare(&engine, NativeInputExport::PortableMir);
    let candidate = prepare(&engine, NativeInputExport::PortableMir);
    let context = context();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime
        .load_program(old_program.program(), Default::default())
        .unwrap();
    let mut backend = Backend::default();
    let old_native = compiler
        .prepare(
            &old_program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &old))
        .unwrap();
    let mut other_runtime = engine.runtime(context.clone());
    let other = other_runtime
        .load_program(candidate.program(), Default::default())
        .unwrap();
    let other_native = compiler
        .prepare(
            &candidate,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut other_runtime, &other))
        .unwrap();
    assert_eq!(backend.calls, 2);
    let selected_old = runtime.select_program(&old).unwrap();
    runtime
        .program(&old)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(candidate.program(), &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    let published = old.clone();
    let selected_published = runtime.select_program(&published).unwrap();
    assert!(
        candidate
            .program()
            .bytecode()
            .same_version(selected_published.verified_program())
    );
    assert!(
        !old_program
            .program()
            .bytecode()
            .same_version(selected_published.verified_program())
    );
    let new_native = compiler
        .prepare(
            &candidate,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| (native_code).install(&mut runtime, &published))
        .unwrap();
    assert_eq!(backend.calls, 2);
    assert!(matches!(
        {
            let native_code = compiler.prepare(
                &old_program,
                ProgramEntry::Main,
                &mut backend,
                &Default::default(),
            );
            native_code.and_then(|native_code| (native_code).install(&mut runtime, &published))
        },
        Err(NativePreparationError::WrongVersion)
    ));
    for jit_policy in [JitPolicy::Enabled, JitPolicy::Disabled] {
        let execution = ExecutionContext {
            jit_policy,
            ..Default::default()
        };
        let mismatch = runtime
            .execute_prepared(&published, ProgramEntry::Main, &[], &execution, &old_native)
            .unwrap_err();
        assert!(
            matches!(mismatch, ref error if error.kind() == RuntimeErrorKind::ModuleValidation && error.to_string().contains("different function or execution version"))
        );
    }
    let stale = runtime
        .execute_prepared_module(
            &selected_old,
            ProgramEntry::Main,
            &old_native,
            runtime.execution_options(),
        )
        .unwrap_err();
    assert!(
        matches!(stale, error if error.to_string().contains("different function or execution version"))
    );
    let PreparedNativeEntry::Native(installed) = &old_native else {
        panic!("expected retained native code")
    };
    assert_eq!(installed.module().key(), selected_old.key());
    assert_eq!(
        runtime
            .execute_prepared_module(
                &selected_published,
                ProgramEntry::Main,
                &new_native,
                runtime.execution_options()
            )
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::Native
    );
    assert_eq!(
        other_runtime
            .execute_prepared(&other, ProgramEntry::Main, &[], &context, &other_native)
            .unwrap()
            .jit
            .unwrap()
            .status,
        JitExecutionStatus::Native
    );
}
