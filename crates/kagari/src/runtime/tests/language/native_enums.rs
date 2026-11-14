#![cfg(feature = "source")]
use crate::{
    BytecodeArtifact,
    engine::{EngineConfig, KagariEngine},
    program::Program,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::module::entry::ProgramEntry;
use kagari_source::source::SourceFile;
use kagari_stdlib as stdlib;
use rstest::rstest;
use std::{fs, path::PathBuf};

#[path = "../../../../tests/support/native_enums.rs"]
mod provider;

mod rejection;

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
    builder.install_all(stdlib::modules().unwrap()).unwrap();
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    builder.config(config);
    builder.install(provider::module().unwrap()).unwrap();
    builder.build().unwrap()
}

fn artifact(engine: &KagariEngine, body: &str) -> DefinitionMetadata<BytecodeArtifact> {
    engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-enums.kgr",
                format!("use external::enums;\n{body}"),
            ),
            Default::default(),
        )
        .unwrap()
}

#[rstest]
fn native_registered_enums_execute_from_serialized_artifacts_with_traced_payloads(
    #[base_dir = "tests/fixtures/language/native_enums"]
    #[files("success/*.kgr")]
    path: PathBuf,
) {
    let body = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read fixture {}: {error}", path.display()));
    let compiler = engine();
    let original = artifact(&compiler, &body);
    let decoded =
        BytecodeArtifact::from_bytes(&original.records().to_bytes(original.definitions()).unwrap())
            .unwrap();
    let program =
        Program::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    // A fresh Engine loads the executable; it never analyzes the source above.
    let mut runtime = engine().runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    runtime.collect_garbage().unwrap();
    let baseline = runtime.gc_stats();
    for _ in 0..3 {
        let report = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .unwrap();
        assert_eq!(report.return_value.as_i32().unwrap(), 42, "{body}");
        drop(report);
        assert!(runtime.execution_root().is_none());
        runtime.collect_garbage().unwrap();
        assert_eq!(
            runtime.gc_stats().root_registrations,
            baseline.root_registrations
        );
        assert_eq!(runtime.gc_stats().objects, baseline.objects);
    }
}
