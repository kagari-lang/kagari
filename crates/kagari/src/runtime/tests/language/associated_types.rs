mod admission;
mod bounds;
mod diagnostics;
mod linking;

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/associated_types"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read associated types fixture {}: {error}",
            path.display()
        )
    });
    let engine = execution::engine(Default::default());
    let checked = engine
        .compile_source(SourceFile::new("associated.kgr", &source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        checked,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}
*/
