use kagari_codegen_cranelift::CraneliftBackend;
use kagari_common::SourceFile;
use kagari_embed::program::PreparedProgram;
use kagari_embed::{BytecodeArtifact, ExecutionContext, JitPolicy, KagariEngine};
use kagari_runtime::value::Value;
use kagari_vm::{JitExecutionStatus, PreparedNativeEntry};

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
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "fallback.kgr",
                "fn main() -> i32 { var value = 40; value += 2; value }",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
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
    assert_eq!(report.return_value, Value::I32(42));
    assert_eq!(
        report.jit.unwrap().status,
        JitExecutionStatus::InterpreterFallback
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}
