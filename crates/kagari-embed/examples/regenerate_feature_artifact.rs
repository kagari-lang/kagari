//! Generate the disposable source-free SDK feature fixture from reviewed source.
use kagari_embed::engine::KagariEngine;
use kagari_source::source::SourceFile;
use std::{fs, path::Path};

// The producer and independent consumer install the exact same public provider.
#[path = "../tests/support/native_provider.rs"]
mod provider;

fn main() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let source = SourceFile::new(
        "memory://feature-artifact.kgr",
        fs::read_to_string(fixtures.join("feature_artifact.kgr")).unwrap(),
    );
    let artifact = KagariEngine::builder()
        .install(provider::module(Default::default()))
        .build()
        .unwrap()
        .compile_to_artifact(source, Default::default())
        .unwrap();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let output = workspace.join("target/fixtures/feature_artifact.kbc");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::write(output, artifact.to_bytes().unwrap()).unwrap();
}
