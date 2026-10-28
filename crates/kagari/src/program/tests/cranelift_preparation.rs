use crate::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::{
        KagariEngine,
        source::{ArtifactOptions, PreparationOptions},
    },
    error::{EmbeddingError, RuntimeFailureKind},
    program::PreparedProgram,
};
use kagari_abi::native_call::JIT_POLL_EXECUTION_SYMBOL;
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_contract::native::ExecutableSafepointKind;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_runtime::value::Value;
use kagari_source::source::SourceFile;
use kagari_types::{scalar::BuiltinType, ty::Ty};
use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};
use rstest::rstest;

#[test]
fn source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("native.kgr", "fn main() -> i32 { 40 + 2 % 5 }"),
            Default::default(),
        )
        .unwrap();
    for encoded in [false, true] {
        let program = if encoded {
            let artifact = BytecodeArtifact::from_bytes(
                &artifact.records().to_bytes(artifact.definitions()).unwrap(),
            )
            .unwrap();
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap()
        } else {
            engine
                .compile_to_program(
                    SourceFile::new("native.kgr", "fn main() -> i32 { 40 + 2 % 5 }"),
                    PreparationOptions {
                        retain_native_input: true,
                        ..Default::default()
                    },
                )
                .unwrap()
        };
        let context = ExecutionContext {
            jit_policy: JitPolicy::Enabled,
            ..Default::default()
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let prepared = runtime
            .prepare_native(
                &program,
                &loaded,
                ProgramEntry::Main,
                &mut CraneliftBackend::for_host().unwrap(),
                &context.cancellation,
            )
            .unwrap();
        let PreparedNativeEntry::Native(installed) = &prepared else {
            panic!("scalar entry should compile to native code");
        };

        drop(program);
        for policy in [
            JitPolicy::Enabled,
            JitPolicy::Disabled,
            JitPolicy::Enabled,
            JitPolicy::CompileOnLoad,
            JitPolicy::CompileOnFirstCall,
            JitPolicy::CompileAfterThreshold(10),
        ] {
            // Scheduling variants still consume explicitly prepared code only.
            let execution = ExecutionContext {
                jit_policy: policy,
                ..Default::default()
            };
            let report = runtime
                .execute_prepared(&loaded, ProgramEntry::Main, &[], &execution, &prepared)
                .unwrap();
            assert_eq!(
                report
                    .return_value
                    .value(runtime.runtime().gc())
                    .expect("retained execution result"),
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
            assert_eq!(runtime.runtime().gc().active_roots(), 0);
        }
    }
}

#[rstest]
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
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let prepared = runtime
        .prepare_native(
            &program,
            &loaded,
            ProgramEntry::Main,
            &mut CraneliftBackend::for_host().unwrap(),
            &context.cancellation,
        )
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
fn decoded_scalar_native_body_completes_automatic_gc_through_abi_safepoints() {
    let engine = KagariEngine::default();
    // Stay inside the backend's straight-line i32 subset. Preserve logical points
    // rather than folding the whole body to one constant before native emission.
    let mut expressions = (1..=64).map(|value| value.to_string()).collect::<Vec<_>>();
    // A balanced expression stays within the ordinary inference-depth limit.
    while expressions.len() > 1 {
        expressions = expressions
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| format!("({} + {})", pair[0], pair[1]))
            .collect();
    }
    let expression = expressions.pop().unwrap();
    let mut options = ArtifactOptions::default();
    options.lowering.optimization = None;
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "native-gc.kgr",
                format!("fn main() -> i32 {{ {expression} }}"),
            ),
            options,
        )
        .unwrap();
    let program = PreparedProgram::from_artifact(
        BytecodeArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    drop(artifact);
    let context = ExecutionContext {
        jit_policy: JitPolicy::Enabled,
        ..Default::default()
    };
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let prepared = runtime
        .prepare_native(
            &program,
            &loaded,
            ProgramEntry::Main,
            &mut CraneliftBackend::for_host().unwrap(),
            &context.cancellation,
        )
        .unwrap();
    let PreparedNativeEntry::Native(installed) = &prepared else {
        panic!("the supported scalar body must compile, never fall back");
    };
    assert!(installed.artifact().safepoints.len() >= 64);
    assert!(installed.artifact().safepoints.iter().all(|point| matches!(
        &point.kind,
        ExecutableSafepointKind::RuntimeHelperCall { helper }
            if helper == JIT_POLL_EXECUTION_SYMBOL
    )));
    drop(program);

    let heap = runtime.runtime().gc();
    let retained = heap.root_execution_values(vec![Value::Unit; 4096]).unwrap();
    const GARBAGE: usize = 4096;
    for _ in 0..GARBAGE {
        heap.alloc_string("garbage".into()).unwrap();
    }
    let before = heap.stats();
    // No explicit collect_garbage or host gc_safepoint occurs in this interval.
    // One entry-boundary step cannot scan the retained root extent, much less
    // reclaim this heap: progress must continue at the emitted native ABI polls.
    let report = runtime
        .execute_prepared(&loaded, ProgramEntry::Main, &[], &context, &prepared)
        .unwrap();
    assert_eq!(report.return_value.value(heap), Some(Value::I32(2080)));
    let jit = report.jit.unwrap();
    assert_eq!(jit.status, JitExecutionStatus::Native);
    assert!(jit.artifact.is_some());
    assert!(jit.diagnostics.is_empty());
    let after = heap.stats();
    assert!(after.collections > before.collections);
    assert!(after.reclaimed_objects - before.reclaimed_objects >= GARBAGE);
    eprintln!(
        "Cranelift native entry: ABI safepoints={}, completed cycles={}, reclaimed objects={}",
        installed.artifact().safepoints.len(),
        after.collections - before.collections,
        after.reclaimed_objects - before.reclaimed_objects,
    );
    assert_eq!(retained.get(heap, 4095), Some(Value::Unit));
    drop(report.return_value);
    drop(retained);
    assert_eq!(heap.active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(!runtime.runtime().is_quarantined());
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
        PreparedProgram::from_artifact(
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
    let old_native = runtime
        .prepare_native(
            &old_program,
            &old,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .unwrap();
    let PreparedNativeEntry::Native(installed_old) = &old_native else {
        panic!("scalar entry should compile to native code");
    };
    let old_entry = installed_old.artifact().code.entry.clone();
    let independent = runtime
        .load_program(&old_program, Default::default())
        .unwrap();
    let independent_native = runtime
        .prepare_native(
            &old_program,
            &independent,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
        .unwrap();
    let selected_old = runtime.runtime().select_program(&old).unwrap();
    let outer = runtime
        .runtime()
        .begin_execution(&selected_old, context.runtime_options())
        .unwrap();
    assert!(
        runtime
            .reload_program(&old, &candidate, Default::default())
            .is_err()
    );
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
        .reload_program(&old, &candidate, Default::default())
        .unwrap();
    let current = old.clone();
    let current_native = runtime
        .prepare_native(
            &candidate,
            &current,
            ProgramEntry::Main,
            &mut backend,
            &Default::default(),
        )
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
