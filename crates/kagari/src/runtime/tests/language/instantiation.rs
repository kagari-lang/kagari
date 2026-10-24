use crate::test_support::execution::{self, Route};
use kagari_source::source::SourceFile;
use rstest::rstest;
use std::{fs, path::PathBuf};

mod contextual;
mod diagnostics;
mod lifecycle;

#[rstest]
fn ordinary_execution(
    #[base_dir = "tests/fixtures/language/instantiation"]
    #[files("*.kgr")]
    path: PathBuf,
) {
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read instantiation fixture {}: {error}",
            path.display()
        )
    });
    let engine = execution::engine(Default::default());
    let artifact = engine
        .compile_source(SourceFile::new("context.kgr", &source))
        .unwrap();
    execution::assert_returns_i32(
        &engine,
        artifact,
        &[Route::Source, Route::Artifact, Route::ArtifactJit],
        42,
    );
}
