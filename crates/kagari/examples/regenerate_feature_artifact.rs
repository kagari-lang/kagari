//! Emit the SDK feature fixtures, or `<source.kgr> <output.kbc>` using stdlib only.
use kagari::engine::KagariEngine;
use kagari_source::source::SourceFile;
use kagari_stdlib::StandardLibrary;
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
    match args.as_slice() {
        [] => {
            let inputs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
            let outputs = workspace.join("target/fixtures");
            emit(
                inputs.join("feature_artifact.kgr"),
                outputs.join("feature_artifact.kbc"),
                true,
            );
            emit(inputs.join("language/conversion_traits/reverse-conversions-call-the-destination-implementation.kgr"), outputs.join("reverse_conversions.kbc"), false);
        }
        [source, output] => emit(source.into(), output.into(), false),
        _ => panic!("usage: regenerate_feature_artifact [<source.kgr> <output.kbc>]"),
    }
}

fn emit(source_path: PathBuf, output: PathBuf, install_fixture_providers: bool) {
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
    let library = StandardLibrary::new(builder.declarations()).unwrap();
    builder.install_all(library.modules().unwrap()).unwrap();
    if install_fixture_providers {
        builder
            .install(provider::module(builder.declarations(), Default::default()))
            .unwrap();
        builder.install(native_enums::module().unwrap()).unwrap();
        builder
            .install(try_carrier::module(library.declarations()).unwrap())
            .unwrap();
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
