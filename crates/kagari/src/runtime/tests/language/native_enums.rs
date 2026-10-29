#![cfg(feature = "source")]
use crate::{
    BytecodeArtifact,
    engine::{EngineConfig, KagariEngine},
    program::PreparedProgram,
};
use kagari_common::identity::metadata::DefinitionMetadata;
use kagari_runtime::{module::entry::ProgramEntry, value::Value};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

#[path = "../../../../tests/support/native_enums.rs"]
mod provider;

mod rejection;

fn engine() -> KagariEngine {
    let mut builder = KagariEngine::builder().unwrap();
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
        PreparedProgram::from_artifact(decoded, &Default::default(), &Default::default()).unwrap();
    // A fresh Engine loads the executable; it never analyzes the source above.
    let mut runtime = engine().runtime(Default::default());
    let loaded = runtime.load_program(&program, Default::default()).unwrap();
    for _ in 0..3 {
        let report = runtime
            .execute(&loaded, ProgramEntry::Main, &[], &Default::default())
            .unwrap();
        assert_eq!(
            report
                .return_value
                .inspect(runtime.runtime(), |view| view.as_i32().map(Value::I32))
                .expect("retained execution result"),
            Value::I32(42),
            "{body}"
        );
        drop(report);
        assert_eq!(runtime.runtime().gc().active_roots(), 0);
        runtime.runtime().collect_garbage().unwrap();
        assert_eq!(runtime.runtime().gc().allocated_objects(), 0);
    }
}
