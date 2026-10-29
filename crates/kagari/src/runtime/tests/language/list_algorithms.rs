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

mod callbacks;
mod rejection;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/list_algorithms"]
    #[files("ordinary/*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::new(config);
    let artifact = engine
        .compile_to_artifact(SourceFile::new("lists.kgr", &source), Default::default())
        .unwrap();
    let decoded =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    assert_eq!(
        runtime
            .execute(&loaded, ProgramEntry::Main, &[], &context)
            .unwrap()
            .return_value
            .inspect(runtime.runtime(), |view| view.as_bool().map(Value::Bool))
            .expect("retained execution result"),
        Value::Bool(true)
    );
    assert_eq!(runtime.runtime().gc().active_roots(), 0);
    // Retire materialized module constants before checking execution-root cleanup.
    runtime
        .reload_program(&loaded, &program, Default::default())
        .unwrap();
    runtime.runtime().collect_garbage().unwrap();
    assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
}
