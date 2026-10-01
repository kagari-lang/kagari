//! Regenerate the typed selected-call product from its application-owned API.
// Test/cross-target sharing keeps the registration authoritative.
#[path = "../tests/fixtures/native_selected_api.rs"]
mod fixture_api;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::Path};

fn main() {
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(fixture_api::dependencies())
        .install(fixture_api::api())
        .build()
        .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-selected.kgr",
                fs::read_to_string(fixtures.join("native_selected.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_selected.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
