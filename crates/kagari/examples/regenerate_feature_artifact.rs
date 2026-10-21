//! Emit the default SDK feature fixture, or `<source.kgr> <output.kbc>` using stdlib only.
use kagari::engine::KagariEngine;
use kagari_source::source::SourceFile;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

// The producer and independent consumer install the exact same public provider.
#[path = "../tests/support/native_enums.rs"]
mod native_enums;
#[path = "../tests/support/native_provider.rs"]
mod provider;
#[path = "../tests/support/try_carrier.rs"]
mod try_carrier;

fn main() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let args: Vec<_> = env::args_os().skip(1).collect();
    let (source_path, output, install_fixture_providers) = match args.as_slice() {
        [] => (
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/feature_artifact.kgr"),
            workspace.join("target/fixtures/feature_artifact.kbc"),
            true,
        ),
        [source, output] => (PathBuf::from(source), PathBuf::from(output), false),
        _ => panic!("usage: regenerate_feature_artifact [<source.kgr> <output.kbc>]"),
    };
    let source_name = if install_fixture_providers {
        "memory://feature-artifact.kgr".to_owned()
    } else {
        source_path.to_string_lossy().into_owned()
    };
    let source = SourceFile::new(
        source_name,
        fs::read_to_string(&source_path).expect("read artifact source"),
    );
    let mut builder = KagariEngine::builder().unwrap();
    if install_fixture_providers {
        builder
            .install(provider::module(Default::default()))
            .unwrap();
        builder.install(native_enums::module().unwrap()).unwrap();
        builder.install(try_carrier::module().unwrap()).unwrap();
    }
    let artifact = builder
        .build()
        .unwrap()
        .compile_to_artifact(source, Default::default())
        .unwrap();
    if let Some(parent) = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(
        output,
        artifact.records().to_bytes(artifact.definitions()).unwrap(),
    )
    .unwrap();
}
