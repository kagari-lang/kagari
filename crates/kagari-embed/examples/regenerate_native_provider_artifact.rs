//! Regenerate the reset provider fixture independently of the full library fixture.
use kagari_common::SourceFile;
use kagari_embed::KagariEngine;
use std::{fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::default();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-provider.kgr",
                fs::read_to_string(fixtures.join("native_provider.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let source = engine
        .native_declaration_sources()
        .into_iter()
        .find(|source| source.uri.ends_with("/array.kgr"))
        .expect("installed array API");
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/array.kgr"),
        source.text,
    )
    .unwrap();
    fs::write(
        fixtures.join("native_provider.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
