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

fn run_failure(source: &str, expected_origin: &str, expected_line: u32, expected_message: &str) {
    let mut config = kagari_embed::EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("result-origin.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for (encoded, jit) in [(false, false), (true, false), (true, true)] {
        let artifact = if encoded {
            BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap()
        } else {
            artifact.clone()
        };
        let mut context = ExecutionContext::default();
        context.capabilities.jit = jit;
        context.language_profile.allow_jit = jit;
        context.jit_policy = if jit {
            kagari_embed::JitPolicy::Enabled
        } else {
            kagari_embed::JitPolicy::Disabled
        };
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        let report = if jit {
            let mut backend = kagari_jit_cranelift::CraneliftBackend::for_host().unwrap();
            runtime.execute_with_backend(&loaded, "main", &[], &context, &mut backend)
        } else {
            runtime.execute(&loaded, "main", &[], &context)
        }
        .unwrap();
        let failure = report.failure.unwrap();
        assert_eq!(failure.message, expected_message);
        assert_eq!(failure.trace.frames[0].function_name, expected_origin);
        assert_eq!(failure.trace.frames[0].line, Some(expected_line));
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        let root = runtime.runtime().root_value(report.return_value).unwrap();
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(
            runtime.runtime().result_failure(&root.value()).unwrap(),
            failure
        );
        drop(root);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(failure.trace.frames[0].function_name, expected_origin);
    }
}

#[test]
fn err_propagation_and_combinators_keep_the_original_stack() {
    run_failure(
        r#"fn origin()->Result<i32,String> {
    Err("original")
}
fn propagate()->Result<i32,String> {val value=origin()?;Ok(value)}
fn main()->Result<i32,String> {
    val errors=[propagate()];
    errors[0].map(|x|x+1).and_then(|x|Ok(x)).map_err(|e|"mapped")
}
"#,
        "origin",
        2,
        "mapped",
    );
}

#[test]
fn reconstructing_err_establishes_a_new_origin() {
    run_failure(
        r#"fn origin()->Result<i32,String>{Err("original")}
fn main()->Result<i32,String>{
    match origin(){Ok(x)=>Ok(x),Err(e)=>Err(e)}
}
"#,
        "main",
        3,
        "original",
    );
}

#[test]
fn option_conversion_creates_an_origin_at_the_conversion() {
    for conversion in ["ok_or(\"missing\")", "ok_or_else(||\"missing\")"] {
        run_failure(
            &format!(
                "fn main()->Result<i32,String>{{\n    val absent:Option<i32> = None;\n    absent.{conversion}\n}}"
            ),
            "main",
            3,
            "missing",
        );
    }
}

#[test]
fn trace_metadata_does_not_participate_in_equality_or_hashing() {
    run_failure(
        r#"fn origin()->Result<i32,String>{Err("same")}
fn main()->Result<i32,String>{
    val a=origin();val b:Result<i32,String> = Err("same");
    std::debug::assert(a==b,"equality ignores trace");
    std::debug::assert(a.hash()==b.hash(),"hash ignores trace");
    val set:MutableSet<Result<i32,String>> = MutableSet::new();set.insert(a);
    std::debug::assert(set.contains(b),"key lookup ignores trace");
    a
}
"#,
        "origin",
        1,
        "same",
    );
}

#[test]
fn failure_stacks_are_bounded_without_losing_the_innermost_origin() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "recursive.kgr",
                r#"
fn recur(n:i32)->Result<i32,String>{if n==0 {Err("deep")}else{recur(n-1)}}
fn main()->Result<i32,String>{recur(160)}
"#,
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let report = runtime.execute(&loaded, "main", &[], &context).unwrap();
    let trace = report.failure.unwrap().trace;
    assert_eq!(
        trace.frames.len(),
        kagari_runtime::error_trace::MAX_ERROR_FRAMES
    );
    assert_eq!(
        trace.omitted_frames,
        162 - kagari_runtime::error_trace::MAX_ERROR_FRAMES
    );
    assert_eq!(trace.frames[0].function_name, "recur");
}

#[test]
fn none_and_handled_errors_do_not_become_execution_failures() {
    for source in [
        "fn main()->Option<i32>{None}",
        "fn main()->Result<i32,String>{Ok(42)}",
        "fn main()->i32{val r:Result<i32,String> = Err(\"handled\");r.unwrap_or(42)}",
    ] {
        let engine = KagariEngine::default();
        let artifact = engine
            .compile_to_artifact(
                SourceFile::new("handled.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap();
        let context = ExecutionContext::default();
        let mut runtime = engine.runtime(context.clone());
        let loaded = runtime.load_program(artifact, Default::default()).unwrap();
        assert!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .failure
                .is_none()
        );
    }
}

#[test]
fn diagnostic_snapshots_survive_reload_without_retaining_script_values() {
    let engine = KagariEngine::default();
    let compile = |source| {
        engine
            .compile_to_artifact(
                SourceFile::new("changing.kgr", source),
                Default::default(),
                Default::default(),
            )
            .unwrap()
    };
    let first = compile("fn main()->Result<i32,String> {Err(\"old\")}");
    let second = compile("fn main()->Result<i32,String> {\n\n    Err(\"new\")\n}");
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let old = runtime.load_program(first, Default::default()).unwrap();
    let report = runtime.execute(&old, "main", &[], &context).unwrap();
    let root = runtime.runtime().root_value(report.return_value).unwrap();
    let failure = report.failure.unwrap();
    let new = runtime
        .reload_program(&old, second, Default::default())
        .unwrap();
    let new_report = runtime
        .execute(&new, "main", &[], &context)
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.trace.frames[0].line, Some(1));
    assert_eq!(new_report.trace.frames[0].line, Some(3));
    assert_ne!(
        failure.trace.frames[0].epoch,
        new_report.trace.frames[0].epoch
    );
    assert_ne!(
        failure.trace.frames[0].code_fingerprint,
        new_report.trace.frames[0].code_fingerprint
    );
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(
        runtime.runtime().result_failure(&root.value()).unwrap(),
        failure
    );
    let raw = root.value();
    drop(root);
    drop(old);
    drop(new);
    runtime.runtime().collect_garbage().unwrap();
    assert!(!runtime.runtime().gc().validate_value(&raw));
    assert!(runtime.runtime().result_failure(&raw).is_none());
    assert_eq!(failure.message, "old");
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
}

#[test]
fn utf8_crlf_and_minimal_artifact_locations_are_portable() {
    let engine = KagariEngine::default();
    let prefix = "    /* 中文😀 */ ";
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "unicode.kgr",
                format!("fn main()->Result<i32,String> {{\r\n{prefix}Err(\"问题\")\r\n}}"),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(artifact.debug.is_none());
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime
        .load_program(
            artifact,
            kagari_embed::LoadOptions {
                module_name: Some("unrelated-load-alias".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let failure = runtime
        .execute(&loaded, "main", &[], &context)
        .unwrap()
        .failure
        .unwrap();
    let origin = &failure.trace.frames[0];
    assert!(origin.source_uri.ends_with("unicode.kgr"));
    assert_eq!(
        (origin.line, origin.column),
        (Some(2), Some(prefix.len() as u32 + 1))
    );
    assert_eq!(failure.message, "问题");
}

#[test]
fn budget_exhaustion_keeps_the_failing_frame_and_releases_resources() {
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "budget.kgr",
                "fn deep()->i32 { var x=0; while x<1000 {x+=1;} x } fn main()->i32 {deep()}",
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let mut context = ExecutionContext::default();
    context.resources.max_instruction_steps = Some(25);
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let error = runtime.execute(&loaded, "main", &[], &context).unwrap_err();
    assert_eq!(error.code(), "KG_RUNTIME_RESOURCE_LIMIT_EXCEEDED");
    assert_eq!(
        error
            .error_trace()
            .unwrap()
            .frames
            .iter()
            .map(|f| f.function_name.as_str())
            .collect::<Vec<_>>(),
        ["deep", "main"]
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    assert!(runtime.runtime().execution_root().is_none());
    assert!(!runtime.runtime().is_quarantined());
    assert_eq!(
        runtime
            .execute(&loaded, "main", &[], &ExecutionContext::default())
            .unwrap()
            .return_value,
        kagari_runtime::value::Value::I32(1000)
    );
}

#[test]
fn diagnostic_previews_do_not_call_user_debug_and_cannot_turn_err_into_a_trap() {
    run_failure(
        r#"struct Problem { val code:i32 }
impl Debug for Problem {fn debug(self)->String {std::debug::assert(false,"must not run");"bad"}}
fn main()->Result<i32,Problem>{Err(Problem {code:7})}
"#,
        "main",
        3,
        "Struct@0:0",
    );
}

#[test]
fn imported_error_frames_keep_their_own_source_locations() {
    use kagari_common::{
        identity::{ModuleIdentity, PackageId},
        source_database::SourceLayer,
    };
    let engine = KagariEngine::default();
    let mut root = None;
    for (name, source) in [
        (
            "origin",
            "pub fn fail()->Result<i32,String>{\n    Err(\"imported\")\n}",
        ),
        (
            "root",
            "use pkg::origin::fail; fn main()->Result<i32,String>{fail()}",
        ),
    ] {
        let uri = format!("mem://{name}");
        engine
            .bind_module(
                &uri,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        let id = engine
            .set_source(&uri, source.into(), SourceLayer::Base)
            .unwrap();
        if name == "root" {
            root = Some(id);
        }
    }
    let checked = engine
        .compile_snapshot(
            engine.source_snapshot(),
            root.unwrap(),
            Default::default(),
            &Default::default(),
        )
        .unwrap();
    let artifact = engine.emit_bytecode(&checked, Default::default()).unwrap();
    let artifact = BytecodeArtifact::from_bytes(&artifact.to_bytes().unwrap()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(artifact, Default::default()).unwrap();
    let trace = runtime
        .execute(&loaded, "main", &[], &context)
        .unwrap()
        .failure
        .unwrap()
        .trace;
    assert_eq!(trace.frames.len(), 2);
    assert_eq!(trace.frames[0].source_uri, "mem://origin");
    assert_eq!(trace.frames[0].line, Some(2));
    assert_eq!(trace.frames[1].source_uri, "mem://root");
    assert_eq!(trace.frames[1].line, Some(1));
}
