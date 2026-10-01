//! Rebuild the direct debug and application host-call proof.
#[path = "../tests/fixtures/native_debug_api.rs"]
mod fixture_api;
use fixture_api::diagnostics;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use kagari_runtime::native::packages::standard_library;
use std::{fs, path::Path};

fn main() {
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .install(diagnostics::native_api())
        .build()
        .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-debug.kgr",
                fs::read_to_string(fixtures.join("native_debug.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_debug.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
