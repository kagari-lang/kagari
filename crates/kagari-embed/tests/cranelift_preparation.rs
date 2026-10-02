use kagari_abi::{scalar::BuiltinType, types::AbiType};
use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::source::SourceFile;
use kagari_embed::{
    BytecodeArtifact,
    context::{ExecutionContext, JitPolicy},
    engine::KagariEngine,
    program::PreparedProgram,
};
use kagari_runtime::value::Value;
use kagari_vm::vm::{JitExecutionStatus, native::PreparedNativeEntry};

#[test]
fn source_and_encoded_mir_use_real_native_code_after_backend_and_program_drop() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("native.kgr", "fn main() -> i32 { 40 + 2 }"),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for encoded in [false, true] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut context = ExecutionContext::default();
        context.language_profile.allow_jit = true;
        context.capabilities.jit = true;
        context.jit_policy = JitPolicy::Enabled;
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let prepared = runtime
            .prepare_native(
                &program,
                &loaded,
                "main",
                &mut CraneliftBackend::for_host().unwrap(),
                &context.cancellation,
            )
            .unwrap();
        assert!(matches!(prepared, PreparedNativeEntry::Native(_)));
        assert_eq!(
            runtime.runtime().resources().counters().instruction_steps,
            0
        );
        drop(program);
        let report = runtime
            .execute_prepared(&loaded, "main", &[], &context, &prepared)
            .unwrap();
        assert_eq!(report.return_value, Value::I32(42));
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
        assert_eq!(
            runtime.runtime().resources().counters().instruction_steps,
            4
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn unsupported_mir_selects_interpreter_before_any_script_instruction() {
    let engine = KagariEngine::default();
    for (source, expected) in [
        ("fn main() -> i32 { var value = 40; value += 2; value }", 42),
        ("fn main() -> i32 { 42 % 5 }", 2),
        (
            "fn main() -> i32 { var count = 40; val next = || { count = count + 1; count }; next(); next() }",
            42,
        ),
    ] {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("fallback.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
        let program =
            PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default())
                .unwrap();
        let mut context = ExecutionContext::default();
        context.language_profile.allow_jit = true;
        context.capabilities.jit = true;
        context.jit_policy = JitPolicy::Enabled;
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(&program, Default::default()).unwrap();
        let prepared = runtime
            .prepare_native(
                &program,
                &loaded,
                "main",
                &mut CraneliftBackend::for_host().unwrap(),
                &context.cancellation,
            )
            .unwrap();
        assert!(matches!(prepared, PreparedNativeEntry::Unsupported { .. }));
        assert_eq!(
            runtime.runtime().resources().counters().instruction_steps,
            0
        );
        let report = runtime
            .execute_prepared(&loaded, "main", &[], &context, &prepared)
            .unwrap();
        assert_eq!(report.return_value, Value::I32(expected));
        assert_eq!(
            report.jit.unwrap().status,
            JitExecutionStatus::InterpreterFallback
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
    }
}

#[test]
fn real_native_entries_keep_their_values_after_reload_and_collect_at_safepoints() {
    use kagari_embed::runtime::KagariRuntime;
    use kagari_runtime::{Runtime, RuntimeConfig, gc::GcHeapConfig};
    let engine = KagariEngine::default();
    let prepare = |value| {
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("versioned.kgr", format!("fn main() -> i32 {{ {value} }}")),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        PreparedProgram::from_artifact(
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap(),
            &Default::default(),
            &Default::default(),
        )
        .unwrap()
    };
    let old_program = prepare(42);
    let candidate = prepare(43);
    let mut context = ExecutionContext::default();
    context.language_profile.allow_jit = true;
    context.capabilities.jit = true;
    context.jit_policy = JitPolicy::Enabled;
    let mut runtime = KagariRuntime::new(
        Runtime::new(RuntimeConfig {
            security: context.security_context(),
            gc: GcHeapConfig {
                collection_threshold: Some(1),
            },
            ..Default::default()
        }),
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
            "main",
            &mut backend,
            &Default::default(),
        )
        .unwrap();
    let outer = runtime
        .runtime()
        .begin_execution(&old, runtime.runtime().execution_options())
        .unwrap();
    let current = runtime
        .reload_program(&old, &candidate, Default::default())
        .unwrap();
    assert_eq!(runtime.runtime().execution_root().unwrap().key(), old.key());
    let current_native = runtime
        .prepare_native(
            &candidate,
            &current,
            "main",
            &mut backend,
            &Default::default(),
        )
        .unwrap();
    drop(backend);
    drop(old_program);
    drop(candidate);
    // Publication and later compilation must not replace the old function's pages.
    let report = runtime
        .execute_prepared(&old, "main", &[], &context, &old_native)
        .unwrap();
    assert_eq!(report.return_value, Value::I32(42));
    assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
    drop(outer);
    for (module, native, value) in [(&old, &old_native, 42), (&current, &current_native, 43)] {
        let dead = runtime
            .runtime()
            .alloc_array(
                module,
                AbiType::Builtin(BuiltinType::I32),
                vec![Value::I32(9)],
            )
            .unwrap();
        let collections = runtime.runtime().gc().stats().collections;
        let report = runtime
            .execute_prepared(module, "main", &[], &context, native)
            .unwrap();
        assert_eq!(report.return_value, Value::I32(value));
        assert_eq!(report.jit.unwrap().status, JitExecutionStatus::Native);
        assert!(runtime.runtime().gc().stats().collections > collections);
        assert!(runtime.runtime().gc().array_len(dead).is_none());
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
    }
}
