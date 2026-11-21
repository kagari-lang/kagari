use crate::test_support::execution::{self, Route};
use kagari_source::source::SourceFile;

mod diagnostics;
mod linking;
mod native_aggregation;

#[test]
fn identity_and_converted_residuals_preserve_the_original_object_or_value() {
    let engine = execution::engine(Default::default());
    let source = include_str!(
        "../../../../tests/fixtures/language/conversion_traits/conversions-can-be-owned-by-the-source-and-preserve-identity.kgr"
    );
    let checked = engine
        .compile_source(SourceFile::new("identity.kgr", source))
        .unwrap();
    execution::assert_returns_i32(&engine, checked, &[Route::Source], 42);
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/conversion_traits"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read conversion trait fixture {}: {error}",
            path.display()
        )
    });
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_source(SourceFile::new("standard-traits.kgr", &source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}
*/
