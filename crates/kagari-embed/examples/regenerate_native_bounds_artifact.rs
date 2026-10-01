//! Generate the independent checked native-bounds product with the same registration.
// Cross-target sharing keeps fixture registration and execution contracts identical.
#[path = "../tests/fixtures/native_bounds_api.rs"]
mod fixture_api;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{cell::Cell, fs, path::Path, rc::Rc};

fn main() {
    let engine = KagariEngine::builder()
        .install(Ok(fixture_api::api(
            fixture_api::module(),
            Rc::new(Cell::new(0)),
        )))
        .build()
        .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-bounds.kgr",
                fs::read_to_string(fixtures.join("native_bounds.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_bounds.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
