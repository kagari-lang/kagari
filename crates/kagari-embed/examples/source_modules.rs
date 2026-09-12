//! Query a source dependency graph without running module initializers.
use kagari_common::{
    cancellation::CancellationToken,
    identity::{ModuleIdentity, PackageId},
    source_database::SourceLayer,
};
use kagari_embed::KagariEngine;

fn main() {
    let engine = KagariEngine::default();
    for (name, text) in [
        ("shared", "pub fn value() -> i32 { 42 }"),
        ("left", "use demo::shared;"),
        ("right", "use demo::shared;"),
        ("root", "use demo::left; use demo::right;"),
    ] {
        let source = format!("memory://{name}");
        engine.bind_module(&source, identity(name)).unwrap();
        engine
            .set_source(&source, text.into(), SourceLayer::Base)
            .unwrap();
    }
    let snapshot = engine
        .analyze(
            engine.source_snapshot(),
            Default::default(),
            &CancellationToken::default(),
        )
        .unwrap();
    for module in snapshot
        .module_graph()
        .initialization_order(&identity("root"), &CancellationToken::default())
        .unwrap()
    {
        println!("{module}");
    }
    // Source bundles still require cross-module linking before execution.
}

fn identity(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("demo".into()),
        path: vec![name.into()],
    }
}
