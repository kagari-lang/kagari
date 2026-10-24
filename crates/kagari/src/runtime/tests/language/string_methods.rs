#![cfg(feature = "source")]
use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod boundaries;
mod rejection;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/string_methods"]
    #[files("ordinary/*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    run(&source, None);
}

fn run(source: &str, expected_error: Option<&str>) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("strings.kgr", source), Default::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    let result = runtime.execute(&loaded, ProgramEntry::Main, &[], &context);
    match expected_error {
        None => assert_eq!(
            result
                .unwrap()
                .return_value
                .value(runtime.runtime().gc())
                .expect("retained execution result"),
            Value::Bool(true)
        ),
        Some(message) => {
            let error = result.unwrap_err();
            assert_eq!(error.code(), "KG_RUNTIME_SCRIPT_TRAP");
            assert!(format!("{error:?}").contains(message), "{error:?}");
        }
    }
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    assert_eq!(
        runtime.runtime().resources().counters().current_call_depth,
        0
    );
    // The published version owns its materialized constants. Replace it with an
    // unused version so this assertion still detects leaked execution roots.
    runtime
        .reload_program(&loaded, &program, Default::default())
        .unwrap();
    assert_eq!(runtime.runtime().collect_garbage().unwrap().live_objects, 0);
    assert!(!runtime.runtime().is_quarantined());
}
