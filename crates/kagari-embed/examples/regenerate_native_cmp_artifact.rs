//! Rebuild the source-free comparison protocol proof with actual providers.
#[path = "../tests/fixtures/native_cmp_api.rs"]
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
                "memory://native-cmp.kgr",
                fs::read_to_string(fixtures.join("native_cmp.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_cmp.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
    let comparison = engine
        .native_declaration_sources()
        .into_iter()
        .find(|source| source.uri == "kagari://native/kagari-std/cmp.kgr")
        .unwrap();
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/cmp.kgr"),
        comparison.text,
    )
    .unwrap();
    let empty = KagariEngine::builder()
        .install_standard_library(false)
        .build()
        .unwrap();
    let artifact = empty
        .compile_to_artifact(
            SourceFile::new(
                "memory://implicit-equality.kgr",
                fs::read_to_string(fixtures.join("implicit_equality.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("implicit_equality.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
