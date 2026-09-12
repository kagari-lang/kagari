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
    let text = "struct Point { var x: i32 }\r\nfn bad() { missing() }\r\nfn good(value: i32) -> i32 { val answer = value + 1; answer }\r\nfn read(p: Point) -> i32 { p.x }\r\ntrait Show { fn show(self) -> i32; }\r\nfn inspect<T: Show>(value: T) -> i32 { value.show() }";
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
    let field = analysis
        .definition_at(text.rfind("p.x").expect("field access") + 2)
        .expect("resolved field");
    println!("field {} -> {:?}", field.name, field.id);
    let annotation = text.find("p: Point").expect("type annotation") + 3;
    let point_type = analysis
        .definition_at(annotation)
        .expect("resolved annotation");
    println!("type {} -> {:?}", point_type.name, point_type.id);
    let constraint = analysis
        .definition_at(text.find("T: Show").expect("trait bound") + 3)
        .expect("resolved trait constraint");
    println!("constraint {} -> {:?}", constraint.name, constraint.id);

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
    assert!(edited.declaration(&field.id).is_some());
    assert_eq!(snapshot.declaration(&target.id), Some(target));
    Ok(())
}
