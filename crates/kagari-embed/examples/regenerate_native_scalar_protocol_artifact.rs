//! Generate source-free scalar protocol evidence from actual registration records.
#[path = "../tests/fixtures/native_scalar_protocol_api.rs"]
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
                "memory://native-scalar-protocol.kgr",
                fs::read_to_string(fixtures.join("native_scalar_protocol.kgr")).unwrap(),
            ),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    fs::write(
        fixtures.join("native_scalar_protocol.kbc"),
        artifact.to_bytes().unwrap(),
    )
    .unwrap();
    for source in engine
        .native_declaration_sources()
        .into_iter()
        .filter(|source| source.uri.ends_with("/hash.kgr") || source.uri.ends_with("/fmt.kgr"))
    {
        let name = source.uri.rsplit('/').next().unwrap();
        fs::write(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../stdlib")
                .join(name),
            source.text,
        )
        .unwrap();
    }
}
