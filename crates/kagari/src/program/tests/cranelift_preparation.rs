#[cfg(feature = "native")]
use crate::program::native::NativeCompiler;
use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{
        KagariEngine,
        source::{ArtifactOptions, PreparationOptions},
    },
    error::{EmbeddingError, RuntimeFailureKind},
    program::{Program, native::NativePreparationError},
    runtime::Runtime as SdkRuntime,
};
use kagari_abi::native::NativeCodeOwner;
use kagari_codegen::{
    BackendConfiguration, BackendFunctionInput, CodegenBackend, diagnostic::BackendCompileError,
};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_contract::native::NativeCompilationProduct;
use kagari_runtime::{
    module::{InstalledProgram, entry::ProgramEntry},
    native::{
        binding::NativeResult, builder::ModuleBuilder, catalog::DeclarationCatalog,
        declarations::FunctionDecl, installation::NativeInstallation, module::NativeModule,
        typed::NativeContext, types::Type,
    },
    value::Value,
};
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
use rstest::rstest;
use std::{
    sync::{
        Arc, Weak,
        atomic::{AtomicI32, Ordering},
    },
    thread,
};

const PLAYER_SOURCE: &str =
    include_str!("../../../tests/fixtures/embedding/native-shared-players.kgr");

fn player_module(catalog: &DeclarationCatalog, value: Arc<AtomicI32>) -> NativeModule {
    let mut builder = ModuleBuilder::new("test::native_player", catalog).unwrap();
    let read = builder
        .define_function(FunctionDecl::new("read").returns(Type::i32()))
        .unwrap();
    builder
        .bind(
            read,
            move |_: &mut NativeContext<'_>, (): ()| -> NativeResult<i32> {
                Ok(value.load(Ordering::SeqCst))
            },
        )
        .unwrap();
    builder.finish().unwrap()
}

/// Observes actual finalized products without changing the backend's output.
struct LifetimeBackend {
    backend: CraneliftBackend,
    pages: Vec<Weak<dyn NativeCodeOwner>>,
}

// SAFETY: every configuration/input/product is delegated unchanged to the real
// Cranelift implementation. Weak page observations grant no execution authority.
unsafe impl CodegenBackend for LifetimeBackend {
    fn configuration(&self) -> BackendConfiguration {
        self.backend.configuration()
    }

    fn compile_function(
        &mut self,
        input: BackendFunctionInput<'_>,
    ) -> Result<NativeCompilationProduct, BackendCompileError> {
        let product = self.backend.compile_function(input)?;
        self.pages.push(Arc::downgrade(&product.owner));
        Ok(product)
    }
}

fn scalar(runtime: &SdkRuntime, loaded: &InstalledProgram, entry: &str) -> i32 {
    let report = runtime
        .execute(loaded, entry, &[], &Default::default())
        .unwrap();
    let Value::I32(value) = report.return_value.value(runtime.runtime().gc()).unwrap() else {
        panic!("scalar player result")
    };
    value
}

#[test]
fn source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop() {
    for encoded in [false, true] {
        let first_value = Arc::new(AtomicI32::new(101));
        let second_value = Arc::new(AtomicI32::new(201));
        let first_host = Arc::downgrade(&first_value);
        let mut builder = KagariEngine::builder().unwrap();
        let first_provider = player_module(builder.declarations(), first_value.clone());
        let second_provider = player_module(builder.declarations(), second_value.clone());
        builder.install(first_provider).unwrap();
        let engine = builder.build().unwrap();
        let prepare = |source: &str| {
            if encoded {
                let artifact = engine
                    .compile_to_artifact(SourceFile::new("native.kgr", source), Default::default())
                    .unwrap();
                let artifact = BytecodeArtifact::from_bytes(
                    &artifact.records().to_bytes(artifact.definitions()).unwrap(),
                )
                .unwrap();
                Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap()
            } else {
                engine
                    .compile_to_program(
                        SourceFile::new("native.kgr", source),
                        PreparationOptions {
                            retain_native_input: true,
                            ..Default::default()
                        },
                    )
                    .unwrap()
            }
        };
        let program = prepare(PLAYER_SOURCE);
        assert_eq!(PLAYER_SOURCE.matches("40 + 2 % 5").count(), 1);
        let replacement = prepare(&PLAYER_SOURCE.replace("40 + 2 % 5", "43"));
        let candidate = program
            .prepare_replacement(&replacement, &Default::default())
            .unwrap();
        let mir = Arc::downgrade(program.state.mir.as_ref().unwrap());
        let context = ExecutionContext {
            jit_policy: JitPolicy::Enabled,
            ..Default::default()
        };
        let mut first = engine.runtime(context.clone());
        let mut modules = kagari_stdlib::modules().unwrap();
        modules.push(second_provider);
        let mut second = SdkRuntime::new(
            NativeInstallation::prepare(&modules)
                .unwrap()
                .runtime(Default::default()),
            context.clone(),
        );
        drop(modules);
        let first_program = first.load_program(&program, Default::default()).unwrap();
        let second_program = second.load_program(&program, Default::default()).unwrap();
        assert!(Arc::ptr_eq(
            first
                .program(&first_program)
                .unwrap()
                .state
                .mir
                .as_ref()
                .unwrap(),
            second
                .program(&second_program)
                .unwrap()
                .state
                .mir
                .as_ref()
                .unwrap()
        ));
        let worker_program = program.clone();
        let worker_candidate = candidate.clone();
        let (native, next_native, pages) = thread::spawn(move || {
            let mut compiler = NativeCompiler::default();
            let mut backend = LifetimeBackend {
                backend: CraneliftBackend::for_host().unwrap(),
                pages: Vec::new(),
            };
            let native = compiler
                .prepare(
                    &worker_program,
                    ProgramEntry::Main,
                    &mut backend,
                    &Default::default(),
                )
                .unwrap();
            let next = compiler
                .prepare(
                    &worker_candidate,
                    ProgramEntry::Main,
                    &mut backend,
                    &Default::default(),
                )
                .unwrap();
            (native, next, backend.pages)
        })
        .join()
        .unwrap();
        assert_eq!(
            pages.len(),
            2,
            "both bodies must compile through real Cranelift"
        );
        let prepared = first.install_native(&first_program, &native).unwrap();
        let independent = second.install_native(&second_program, &native).unwrap();
        let PreparedNativeEntry::Native(installed) = &prepared else {
            panic!("native first player")
        };
        let PreparedNativeEntry::Native(other) = &independent else {
            panic!("native second player")
        };
        assert_eq!(installed.artifact().code.entry, other.artifact().code.entry);
        drop(program);
        drop(engine);
        for policy in [
            JitPolicy::Enabled,
            JitPolicy::Disabled,
            JitPolicy::Enabled,
            JitPolicy::CompileOnLoad,
            JitPolicy::CompileOnFirstCall,
            JitPolicy::CompileAfterThreshold(10),
        ] {
            let execution = ExecutionContext {
                jit_policy: policy,
                ..Default::default()
            };
            let report = first
                .execute_prepared(
                    &first_program,
                    ProgramEntry::Main,
                    &[],
                    &execution,
                    &prepared,
                )
                .unwrap();
            assert_eq!(
                report.return_value.value(first.runtime().gc()).unwrap(),
                Value::I32(42)
            );
            let jit = report.jit.unwrap();
            assert_eq!(jit.backend, installed.artifact().code.backend);
            assert_eq!(jit.function, installed.artifact().function);
            if policy == JitPolicy::Disabled {
                assert_eq!(jit.status, JitExecutionStatus::InterpreterFallback);
                assert!(jit.artifact.is_none());
                assert_eq!(
                    jit.diagnostics,
                    ["JIT disabled by execution context policy"]
                );
            } else {
                assert_eq!(jit.status, JitExecutionStatus::Native);
                assert!(jit.artifact.is_some());
                assert!(jit.diagnostics.is_empty());
            }
            drop(report.return_value);
            assert_eq!(first.runtime().gc().active_roots(), 0);
        }
        assert_eq!(scalar(&first, &first_program, "script_next"), 1);
        assert_eq!(scalar(&first, &first_program, "host_value"), 101);
        assert_eq!(scalar(&second, &second_program, "host_value"), 201);
        first
            .replace_program(&first_program, &candidate, Default::default())
            .unwrap();
        let updated = first.install_native(&first_program, &next_native).unwrap();
        assert_eq!(scalar(&first, &first_program, "script_next"), 2);
        assert_eq!(scalar(&second, &second_program, "script_next"), 1);
        first_value.store(102, Ordering::SeqCst);
        assert_eq!(scalar(&first, &first_program, "host_value"), 102);
        assert_eq!(scalar(&second, &second_program, "host_value"), 201);
        let second_before = second.runtime().publication_token(&second_program).unwrap();
        assert!(matches!(
            second.install_native(&second_program, &next_native),
            Err(NativePreparationError::WrongVersion)
        ));
        assert_eq!(
            second.runtime().publication_token(&second_program).unwrap(),
            second_before
        );
        for (runtime, loaded, entry, value) in [
            (&first, &first_program, &updated, 43),
            (&second, &second_program, &independent, 42),
        ] {
            let report = runtime
                .execute_prepared(loaded, ProgramEntry::Main, &[], &context, entry)
                .unwrap();
            assert_eq!(
                report.return_value.value(runtime.runtime().gc()).unwrap(),
                Value::I32(value)
            );
            assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
        }
        drop((native, next_native, prepared, updated));
        first.close().unwrap();
        drop(first);
        drop(first_value);
        assert!(
            first_host.upgrade().is_none(),
            "shared code must not retain another player's Rust capture"
        );
        assert!(
            pages[0].upgrade().is_some(),
            "second player still owns the old pages"
        );
        assert!(
            pages[1].upgrade().is_none(),
            "failed delivery must not retain the new pages in player two"
        );
        assert!(mir.upgrade().is_some());
        second_value.store(202, Ordering::SeqCst);
        assert_eq!(scalar(&second, &second_program, "host_value"), 202);
        let report = second
            .execute_prepared(
                &second_program,
                ProgramEntry::Main,
                &[],
                &context,
                &independent,
            )
            .unwrap();
        assert_eq!(
            report.return_value.value(second.runtime().gc()).unwrap(),
            Value::I32(42)
        );
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
        drop(report.return_value);
        assert_eq!(second.runtime().gc().active_roots(), 0);
        assert_eq!(
            second.runtime().resources().counters().current_call_depth,
            0
        );
        drop(independent);
        second.close().unwrap();
        drop(second);
        assert!(pages.iter().all(|page| page.upgrade().is_none()));
        assert!(
            mir.upgrade().is_none(),
            "candidate identity must not retain predecessor MIR"
        );
    }
}

#[rstest]
#[case::ordinary_call("fn helper() -> i32 { 42 } fn main() -> i32 { helper() }", 42)]
#[case::mutable_local("fn main() -> i32 { var value = 40; value += 2; value }", 42)]
#[case::remainder("fn main() -> i32 { 42 % 5 }", 2)]
#[case::capturing_closure(
    "fn main() -> i32 { var count = 40; val next = || { count = count + 1; count }; next(); next() }",
    42
)]
fn unsupported_mir_selects_interpreter_before_any_script_instruction(
    #[case] source: &str,
    #[case] expected: i32,
) {
    let engine = KagariEngine::default();
    // Preserve unsupported MIR operations for this backend-boundary fixture.
    // The native test above separately covers default constant folding.
    let mut options = ArtifactOptions::default();
    options.lowering.optimization = None;
    let artifact = engine
        .compile_to_artifact(SourceFile::new("fallback.kgr", source), options)
        .unwrap();
    let artifact =
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let prepared = NativeCompiler::default()
        .prepare(
            &program,
            ProgramEntry::Main,
            &mut CraneliftBackend::for_host().unwrap(),
            &context.cancellation,
        )
        .and_then(|native_code| runtime.install_native(&loaded, &native_code))
        .unwrap();
    assert!(matches!(prepared, PreparedNativeEntry::Unsupported { .. }));

    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(expected)
    );
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
    drop(report.return_value);
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn real_native_entries_keep_their_values_after_reload_and_collect_at_safepoints() {
    use crate::runtime::Runtime;
    use kagari_runtime::{Runtime as CoreRuntime, RuntimeConfig, gc::GcHeapConfig};
    let engine = KagariEngine::default();
    let prepare = |value| {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("versioned.kgr", format!("fn main() -> i32 {{ {value} }}")),
                Default::default(),
            )
            .unwrap();
        Program::from_artifact(
            BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap()
    };
    let old_program = prepare(42);
    let candidate = prepare(43);
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    let mut runtime = Runtime::new(
        {
            let mut registered = CoreRuntime::new(RuntimeConfig {
                gc: GcHeapConfig {
                    collection_threshold: Some(1),
                    ..Default::default()
                },
                ..Default::default()
            });
            kagari_runtime::native::module::NativeModule::install_all(
                &kagari_stdlib::modules().unwrap(),
                &mut registered,
            )
            .unwrap();
            registered
        },
        context.clone(),
    );
    let old = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let mut backend = CraneliftBackend::for_host().unwrap();
    let old_native = NativeCompiler::default()
        .prepare(
            &old_program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| runtime.install_native(&old, &native_code))
        .unwrap();
    let PreparedNativeEntry::Native(installed_old) = &old_native else {
        panic!("scalar entry should compile to native code");
    };
    let old_entry = installed_old.artifact().code.entry.clone();
    let independent = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let independent_native = NativeCompiler::default()
        .prepare(
            &old_program,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| runtime.install_native(&independent, &native_code))
        .unwrap();
    let selected_old = runtime.runtime().select_program(&old).unwrap();
    let outer = runtime
        .runtime()
        .begin_execution(&selected_old, context.runtime_options())
        .unwrap();
    // The live session's Runtime borrow statically excludes mutable publication.
    // Dynamic admission for forgotten scopes and callbacks is tested at its owner.
    assert_eq!(
        runtime.runtime().select_program(&old).unwrap().key(),
        selected_old.key()
    );
    assert_eq!(
        runtime.runtime().execution_root().unwrap().key(),
        selected_old.key()
    );
    // Rejected publication must not replace the admitted function's pages.
    let report = runtime
        .backend()
        .execute_prepared(&selected_old, ProgramEntry::Main, &old_native)
        .unwrap();
    assert_eq!(
        report
            .return_value
            .value(runtime.runtime().gc())
            .expect("retained execution result"),
        Value::I32(42)
    );
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    drop(report.return_value);
    drop(outer);
    runtime
        .program(&old)
        .and_then(|base| base.prepare_replacement(&candidate, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&old, &candidate, Default::default()))
        .unwrap();
    let current = old.clone();
    let current_native = NativeCompiler::default()
        .prepare(
            &candidate,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .and_then(|native_code| runtime.install_native(&current, &native_code))
        .unwrap();
    drop(backend);
    drop(old_program);
    drop(candidate);
    assert_eq!(installed_old.module().key(), selected_old.key());
    assert_eq!(installed_old.artifact().code.entry, old_entry);
    // Retaining code memory is not authority to invoke an obsolete named body.
    // Reject its product even when the host requests interpreter fallback.
    for jit_policy in [JitPolicy::Enabled, JitPolicy::Disabled] {
        let execution = ExecutionContext {
            jit_policy,
            ..Default::default()
        };
        let error = runtime
            .execute_prepared(&old, ProgramEntry::Main, &[], &execution, &old_native)
            .unwrap_err();
        assert!(matches!(error, EmbeddingError::Runtime {
            kind: RuntimeFailureKind::BytecodeVerification, message, ..
        } if message.contains("different function or execution version")));
    }
    // The independent installation still selects the old product legitimately;
    // the updated installation selects the new one. Both must run actual native code.
    let selected_independent = runtime.runtime().select_program(&independent).unwrap();
    let selected_current = runtime.runtime().select_program(&current).unwrap();
    for (module, native, value) in [
        (&selected_independent, &independent_native, 42),
        (&selected_current, &current_native, 43),
    ] {
        let _session = runtime
            .runtime()
            .begin_execution(module, context.runtime_options())
            .unwrap();
        let dead = runtime
            .runtime()
            .alloc_array(module, Ty::Builtin(BuiltinType::I32), vec![Value::I32(9)])
            .unwrap();
        let collections = runtime.runtime().gc().stats().collections;
        let report = runtime
            .backend()
            .execute_prepared(module, ProgramEntry::Main, native)
            .unwrap();
        assert_eq!(
            report
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::I32(value)
        );
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
        drop(report.return_value);
        assert!(runtime.runtime().gc().stats().collections > collections);
        assert!(runtime.runtime().gc().array_len(dead).is_none());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}
