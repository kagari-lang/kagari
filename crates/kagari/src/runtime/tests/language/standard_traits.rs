mod artifact_validation;
mod callback_lifecycle;
mod diagnostics;
mod hash_stability;
mod linking;

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
fn ordinary_programs(
    #[base_dir = "tests/fixtures/language/standard_traits"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read standard trait fixture {}: {error}",
            path.display()
        )
    });
    let mut config = EngineConfig::default();
    config.default_runtime.gc.collection_threshold = Some(1);
    let engine = execution::engine(config);
    let artifact = engine
        .compile_source(SourceFile::new("standard-traits.kgr", source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}
*/
