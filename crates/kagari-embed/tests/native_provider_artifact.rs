//! Provider execution must not require the source feature or a native backend.
use kagari_bytecode::artifact::KbcArtifact;
use kagari_runtime::{native::packages::standard_library, value::Value};

use kagari_embed::{
    context::ExecutionContext,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};

const ARTIFACT: &[u8] = include_bytes!("fixtures/native_provider.kbc");

#[test]
fn unsupported_wire_versions_are_rejected_before_product_decoding() {
    let current = KbcArtifact::from_bytes(ARTIFACT)
        .unwrap()
        .header
        .format_version;
    for version in [current - 1, current + 1] {
        let mut bytes = ARTIFACT.to_vec();
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        assert!(KbcArtifact::from_bytes(&bytes).is_err());
    }
}

#[test]
fn provider_artifact_executes_and_releases_callback_roots() {
    let program = PreparedProgram::from_artifact(
        KbcArtifact::from_bytes(ARTIFACT).unwrap(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    let context = ExecutionContext::default();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let mut runtime = KagariEngine::new(config).runtime(context.clone());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        assert_eq!(
            runtime
                .execute(&loaded, "main", &[], &context)
                .unwrap()
                .return_value,
            Value::I32(42)
        );
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        assert_eq!(
            runtime.runtime().resources().counters().current_call_depth,
            0
        );
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}

#[cfg(feature = "source")]
#[test]
fn provider_fixture_matches_source_emission() {
    use kagari_common::source::SourceFile;
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-provider.kgr",
                include_str!("fixtures/native_provider.kgr"),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let declarations: Vec<_> = artifact
        .program
        .modules
        .iter()
        .flat_map(|module| module.native_declarations.iter().cloned())
        .collect();
    let api = standard_library();
    let registered: Vec<_> = api
        .modules()
        .iter()
        .flat_map(|module| module.native_declarations())
        .collect();
    assert_eq!(declarations, registered);
    assert_eq!(artifact.to_bytes().unwrap(), ARTIFACT);
}
