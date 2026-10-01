//! Rebuild the bounded typed-default product from its real Rust registration.
#[path = "../tests/fixtures/native_default_typed_api.rs"]
mod fixture_api;
use fixture_api::defaults;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::PathBuf};

fn main() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(defaults::native_api())
        .build()
        .unwrap();
    let source = fs::read_to_string(fixtures.join("native_default_typed.kgr")).unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new("memory://native-default-typed.kgr", source),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_default_typed.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
