//! Rebuild a source-independent proof of native range and enum alias authoring.
#[path = "../tests/fixtures/native_ops_api.rs"]
pub mod fixture_api;
use fixture_api::shapes;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::Path};

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(shapes::native_api())
        .build()
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-ops.kgr",
                fs::read_to_string(fixtures.join("native_ops.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_ops.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
