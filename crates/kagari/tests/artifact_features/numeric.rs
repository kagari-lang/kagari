//! Artifact-only entrypoint selected by scripts/check_features.py --numeric-only.
//! Kept outside the workspace test targets so a disposable product is never optional.
use kagari::{
    BytecodeArtifact,
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use std::{env, fs};

#[test]
fn source_free_numeric_sampling_executes_and_releases_roots() {
    let path = env::var_os("KAGARI_NUMERIC_ARTIFACT")
        .expect("run scripts/check_features.py --numeric-only to produce the numeric artifact");
    let bytes = fs::read(path).expect("read the freshly emitted numeric artifact");
    let artifact = BytecodeArtifact::from_bytes(&bytes).unwrap();
    let program =
        PreparedProgram::from_artifact(artifact, &Default::default(), &Default::default()).unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut builder = KagariEngine::builder().unwrap();
    builder.config(config);
    let engine = builder.build().unwrap();
    let context = ExecutionContext::default();
    let mut runtime = engine.runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    // main and the exported checksum both include exact PRNG and tolerant geometry checks.
    for (entry, expected) in [
        (ProgramEntry::Main, Value::I32(42)),
        (
            ProgramEntry::Export("sampling_checksum"),
            Value::U64(0x396d503bb05c761a),
        ),
    ] {
        let result = runtime.execute(&loaded, entry, &[], &context).unwrap();
        assert_eq!(
            result.return_value.value(runtime.runtime().gc()).unwrap(),
            expected,
            "{entry:?}",
        );
        drop(result);
        assert_eq!(runtime.runtime().gc().active_roots(), 0, "{entry:?}");
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0,
            "{entry:?}",
        );
        assert!(!runtime.runtime().is_quarantined(), "{entry:?}");
    }
}
