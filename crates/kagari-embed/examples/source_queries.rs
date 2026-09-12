//! Run with `cargo run -p kagari-embed --example source_queries`.
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    line_index::PositionEncoding,
    source_database::SourceLayer,
};
use kagari_embed::KagariEngine;

fn main() -> kagari_embed::CompileResult<()> {
    let engine = KagariEngine::default();
    let source_name = "editor://game/main.kgr";
    engine.bind_module(
        source_name,
        ModuleIdentity {
            package: PackageId("game".into()),
            path: vec!["main".into()],
        },
    )?;
    // An erroneous neighbor does not prevent navigation in the correct function.
    let text =
        "fn bad() { missing() }\r\nfn good(value: i32) -> i32 { val answer = value + 1; answer }";
    let file = engine.set_source(source_name, text.into(), SourceLayer::Overlay)?;
    let snapshot = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    let analysis = snapshot.file(file).expect("source belongs to snapshot");
    let offset = text.rfind("answer }").expect("reference offset");
    let target = analysis.definition_at(offset).expect("resolved local");
    let position = analysis
        .source()
        .position(target.location.range.start, PositionEncoding::Utf16)
        .expect("source position");
    println!(
        "{}:{}:{} -> {} ({:?})",
        source_name,
        position.line + 1,
        position.character + 1,
        target.name,
        target.id
    );
    for binding in analysis.visible_bindings(offset) {
        println!("{}: {:?}", binding.declaration.name, binding.ty);
    }

    engine.set_source(
        source_name,
        text.replace("value + 1", "value + 2"),
        SourceLayer::Overlay,
    )?;
    let edited = engine.analyze(
        engine.source_snapshot(),
        Default::default(),
        &Default::default(),
    )?;
    assert!(edited.declaration(&target.id).is_none());
    assert_eq!(snapshot.declaration(&target.id), Some(target));
    Ok(())
}
