#![cfg(feature = "source")]
use crate::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{
    error::reason::{ErrorReason, IndexError},
    module::entry::ProgramEntry,
    reload::ReloadValidationError,
};
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;

mod boundaries;
mod rejection;

// Temporarily paused: stdlib restoration (NC01-NC06).
// #[rstest]
// fn ordinary_execution(
//     #[base_dir = "tests/fixtures/language/string_methods"]
//     #[files("ordinary/*.kgr")]
//     path: PathBuf,
// ) {
//     let source = fs::read_to_string(&path)
//         .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
//     run(&source, None);
// }

fn run(source: &str, expected_error: Option<&str>) {
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = KagariEngine::with_native_modules(config, stdlib::modules().unwrap()).unwrap();
    let artifact = engine
        .compile_to_artifact(SourceFile::new("strings.kgr", source), Default::default())
        .unwrap();
    let artifact =
        KbcArtifact::from_bytes(&artifact.records().to_bytes(artifact.definitions()).unwrap())
            .unwrap();
    let program =
        Program::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    let result = runtime.execute(&loaded, ProgramEntry::Main, &[], &context);
    match expected_error {
        None => assert!(result.unwrap().return_value.as_bool().unwrap()),
        Some(message) => {
            let error = result.unwrap_err();
            assert_eq!(error.code(), "KG_RUNTIME_INDEX_OUT_OF_BOUNDS");
            assert_eq!(
                error.reason(),
                &ErrorReason::Index(IndexError::Provider(message.to_owned()))
            );
        }
    }
    runtime.collect_garbage().unwrap();
    assert_eq!(
        runtime.gc_stats().root_registrations,
        baseline.root_registrations
    );
    assert_eq!(runtime.resources().counters().current_call_depth, 0);
    // The published version owns its materialized constants. Replace it with an
    // unused version so this assertion still detects leaked execution roots.
    runtime
        .program(&loaded)
        .map_err(ReloadValidationError::Runtime)
        .and_then(|base| base.prepare_replacement(&program, &Default::default()))
        .and_then(|candidate| runtime.replace_program(&loaded, &candidate, Default::default()))
        .unwrap();
    runtime.collect_garbage().unwrap();
    assert_eq!(runtime.gc_stats().objects, baseline.objects);
    assert!(!runtime.is_quarantined());
}
