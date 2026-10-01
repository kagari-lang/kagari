//! Generate portable primitive method/parsing proofs from actual native providers.
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use kagari_runtime::native::packages::standard_library;
use std::{fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .build()
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-numeric.kgr",
                fs::read_to_string(fixtures.join("native_numeric.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_numeric.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
