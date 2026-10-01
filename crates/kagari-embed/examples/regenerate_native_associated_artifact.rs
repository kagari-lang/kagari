//! Regenerate the independent associated-output product using its registered API.
// Cross-target fixture sharing keeps declarations and executable handlers identical.
#[path = "../tests/fixtures/native_associated_api.rs"]
mod fixture_api;
use fixture_api::typed;
use kagari_common::source::SourceFile;
use kagari_embed::engine::KagariEngine;
use std::{cell::Cell, fs, path::Path, rc::Rc};
fn main() {
    let engine = KagariEngine::builder()
        .install(Ok(fixture_api::api(
            fixture_api::module(),
            Rc::new(Cell::new(0)),
        )))
        .install(typed::native_api())
        .build()
        .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let artifact = engine
        .compile_to_artifact(
            SourceFile::new(
                "memory://native-associated.kgr",
                fs::read_to_string(fixtures.join("native_associated.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_associated.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
}
