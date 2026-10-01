//! Regenerate the reset provider fixture independently of the full library fixture.
use kagari_common::SourceFile;
use kagari_embed::KagariEngine;
use std::{fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = KagariEngine::default()
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-provider.kgr",
                fs::read_to_string(fixtures.join("native_provider.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_provider.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
