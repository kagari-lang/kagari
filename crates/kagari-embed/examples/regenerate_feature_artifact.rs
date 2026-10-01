//! Regenerate the canonical source-free SDK feature fixture from its exact source.
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = SourceFile::new(
        "memory://feature-artifact.kgr",
        fs::read_to_string(fixtures.join("feature_artifact.kgr")).unwrap(),
    );
    let artifact = KagariEngine::default()
        .compile_to_artifact(source, Default::default(), Default::default())
        .unwrap();
    fs::write(
        fixtures.join("feature_artifact.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
