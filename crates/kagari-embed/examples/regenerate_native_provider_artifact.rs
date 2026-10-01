//! Regenerate the reset provider fixture independently of the full library fixture.
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
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
    for source in engine.native_declaration_sources() {
        let module = source
            .uri
            .strip_prefix("kagari://native/kagari-std/")
            .unwrap();
        fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../stdlib")
                .join(module),
            source.text,
        )
        .unwrap();
    }
    fs::write(
        fixtures.join("native_provider.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
