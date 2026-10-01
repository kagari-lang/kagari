//! Rebuild the bounded external-default product from actual registered providers.
#[path = "../tests/fixtures/native_default_external_api.rs"]
pub mod fixture_api;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{fs, path::PathBuf};

fn main() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let engine = KagariEngine::builder()
        .install_standard_library(false)
        .install(fixture_api::api(false))
        .build()
        .unwrap();
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-default-external.kgr",
                fs::read_to_string(fixtures.join("native_default_external.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_default_external.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
