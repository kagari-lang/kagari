//! Regenerate the application-owned returned-state proof using actual registrations.
#[path = "../tests/fixtures/native_state_api.rs"]
pub mod fixture_api;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::Path};
fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(fixture_api::api())
        .build()
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-state.kgr",
                fs::read_to_string(fixtures.join("native_state.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_state.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
