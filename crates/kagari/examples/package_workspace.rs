//! Load one source package closure and explicitly call several public entries.

use kagari::engine::KagariEngine;
use kagari_common::cancellation::CancellationToken;
use kagari_package::{
    filesystem::FilesystemSources,
    project::{ProjectInputs, ProjectOptions},
};
use std::path::PathBuf;

fn main() {
    let directory =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/package-workspace");
    let sources = FilesystemSources::new(&directory).expect("approved source directory");
    let cancel = CancellationToken::default();
    let project = ProjectInputs::load(
        &sources,
        "",
        &ProjectOptions {
            // A host can supply a namespace; if also declared, both must agree.
            namespace: Some("examples/game".into()),
            ..Default::default()
        },
        &cancel,
    )
    .expect("package project");
    let engine = KagariEngine::default();
    let checked = engine
        .compile_package(&project.programs()["gameplay"], &cancel)
        .expect("checked package");
    let program = engine
        .prepare(&checked, Default::default())
        .expect("verified program");
    let mut runtime = engine.runtime(Default::default());
    let loaded = runtime
        .load_program(&program, Default::default())
        .expect("loaded program");
    let context = Default::default();
    let login = runtime
        .execute_typed::<(), i32>(&loaded, "on_login", (), &context)
        .expect("public entry");
    let tick = runtime
        .execute_typed::<(), i32>(&loaded, "tick", (), &context)
        .expect("public entry");
    let damage = runtime
        .execute_typed::<(i32,), i32>(&loaded, "damage", (21,), &context)
        .expect("public alias");
    assert_eq!((login, tick, damage), (40, 2, 42));
    println!("on_login={login}, tick={tick}, damage(21)={damage}");
}
