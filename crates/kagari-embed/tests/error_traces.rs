use kagari_common::SourceFile;
use kagari_embed::{BytecodeArtifact, ExecutionContext, KagariEngine};

#[test]
fn interpreter_traps_capture_frames_and_original_source_locations() {
    let engine = KagariEngine::default();
    let artifact=engine.compile_to_artifact(SourceFile::new("origin.kgr","fn fail()->i32 {\n    42/0\n}\nfn middle()->i32 {fail()}\nfn main()->i32 {middle()}\n"),Default::default(),Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    let trace = error.error_trace().unwrap();
    assert_eq!(
        trace
            .frames
            .iter()
            .map(|f| f.function_name.as_str())
            .collect::<Vec<_>>(),
        ["fail", "middle", "main"]
    );
    assert!(trace.frames[0].source_uri.ends_with("origin.kgr"));
    assert_eq!(trace.frames[0].line, Some(2));
    assert_eq!(trace.frames[0].column, Some(5));
    assert_eq!(trace.frames[1].line, Some(4));
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn native_overflow_reports_the_same_instruction_as_the_interpreter() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "native-origin.kgr",
                "fn main()->i32 {\n    val large=2147483647;\n    large+1\n}",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut traces = vec![];
    for jit in [false, true] {
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime
            .load_program(artifact.clone(), Default::default())
            .unwrap();
        let error = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap_err();
        traces.push(error.error_trace().unwrap().clone());
    }
    assert_eq!(traces[0].frames[0].line, Some(3));
    assert_eq!(traces[0], traces[1]);
}
