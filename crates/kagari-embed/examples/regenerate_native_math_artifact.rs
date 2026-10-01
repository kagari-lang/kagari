//! Regenerate the complete portable math proof from ordinary registered providers.
// Test/cross-target sharing keeps application registration authoritative.
#[path = "../tests/fixtures/native_math_api.rs"]
mod fixture_api;
use fixture_api::numbers;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use kagari_runtime::native::packages::standard_library;
use std::{fs, path::Path};

fn main() {
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(Ok(standard_library()))
        .install(numbers::native_api())
        .build()
        .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-math-complete.kgr",
                fs::read_to_string(fixtures.join("native_math.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_math.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
