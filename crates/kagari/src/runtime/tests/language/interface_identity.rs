use crate::{engine::EngineConfig, test_support::execution};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod admission;
mod diagnostics;
mod lifecycle;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/interface_identity"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read interface identity fixture {}: {error}",
            path.display()
        )
    });
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_source(SourceFile::new("interface-identity.kgr", &source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[
            execution::Route::Source,
            execution::Route::Artifact,
            execution::Route::ArtifactJit,
        ],
        42,
    );
}
