//! Regenerate the independent sorting product and the array tooling view.
#[path = "../tests/fixtures/native_sorting_api.rs"]
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
                "memory://native-sorting.kgr",
                fs::read_to_string(fixtures.join("native_sorting.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_sorting.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
    let array = engine
        .native_declaration_sources()
        .into_iter()
        .find(|source| source.uri == "kagari://native/kagari-std/array.kgr")
        .unwrap();
    fs::write(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stdlib/array.kgr"),
        array.text,
    )
    .unwrap();
}
