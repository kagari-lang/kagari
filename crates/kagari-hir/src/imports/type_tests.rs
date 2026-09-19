use super::tests::{analyze, insert};
use crate::{
    analysis::AnalysisDatabase,
    types::{BuiltinType, TypeId},
};
use kagari_common::{
    DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

#[test]
fn imported_annotations_preserve_nominal_identity_and_definition_locations() {
    let mut db = SourceDatabase::default();
    let types = insert(
        &mut db,
        "types",
        "pub struct Data { val value: i32 } pub enum Choice { First } pub trait View { fn read(self) -> i32; }",
    );
    let text = "use pkg::types as lib; use pkg::types::Data as D; use pkg::types::Choice; use pkg::types::View; struct Holder { val items: [D] } fn pass(x: D) -> lib::Data { val y: lib::Data = x; y } fn choice(x: Choice) -> Choice { x } fn view(x: View) -> View { x }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(types)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(signature.params[0].ty, signature.return_type);
    assert!(matches!(&signature.return_type, TypeId::Struct(id) if id.module.path == ["types"]));
    for needle in [
        "[D]",
        "D)",
        "lib::Data {",
        "lib::Data =",
        "Choice)",
        "View)",
    ] {
        let offset = text.find(needle).unwrap() + usize::from(needle == "[D]");
        let declaration = snapshot.definition_at(root, offset).unwrap();
        assert_eq!(declaration.location.file, types);
        assert!(db.snapshot().contains(declaration.location));
    }
}

#[test]
fn exported_signatures_use_imported_types_before_callers_are_checked() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "models", "pub struct Data { val value: i32 }");
    let api = insert(
        &mut db,
        "api",
        "use pkg::models::Data; pub fn pass(x: Data) -> Data { x }",
    );
    let text = "use pkg::api::pass; use pkg::models::Data; fn forward(x: Data) -> Data { pass(x) }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    for (_, node) in snapshot.module_graph().modules() {
        let diagnostics = snapshot.file(node.file).unwrap().result().diagnostics();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }
    let file = snapshot.file(root).unwrap();
    let imported = file
        .source_function_at(text.find("pass(x)").unwrap())
        .unwrap();
    assert_eq!(imported.id.file, api);
    let local = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "forward")
        .unwrap();
    assert_eq!(local.return_type, imported.signature.return_type);
    assert_eq!(local.params[0].ty, imported.signature.params[0].ty);
}

#[test]
fn shared_facade_resolution_rejects_stale_targets_and_terminates_cycles() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let text = "use pkg::types::Data;";
    let root = insert(&mut db, "root", text);
    let first = analyze(&db);
    let target = first
        .source_import_at(root, text.find("pkg::types").unwrap())
        .unwrap();
    assert!(
        first
            .module_graph()
            .resolve_item(target.clone(), &Default::default())
            .unwrap()
            .is_some()
    );
    db.set(
        "mem://types",
        "pub struct Data { val value: bool }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analyze(&db);
    assert!(
        second
            .module_graph()
            .resolve_item(target.clone(), &Default::default())
            .unwrap()
            .is_none()
    );
    let cancel = kagari_common::cancellation::CancellationToken::default();
    cancel.cancel();
    assert!(first.module_graph().resolve_item(target, &cancel).is_err());
    let a = insert(&mut db, "a", "pub use pkg::b::Alias;");
    insert(&mut db, "b", "pub use pkg::a::Alias;");
    let cycle = analyze(&db);
    let target = cycle.source_import_at(a, "pub use ".len()).unwrap();
    assert!(
        cycle
            .module_graph()
            .resolve_item(target, &Default::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn type_facades_resolve_before_signatures_including_module_aliases() {
    let mut db = SourceDatabase::default();
    let types = insert(&mut db, "types", "pub struct Data { val value: i32 }");
    insert(
        &mut db,
        "facade",
        "pub use pkg::types::Data as Renamed; pub use pkg::types as models;",
    );
    let text = "use pkg::facade::Renamed as D; use pkg::facade::models as m; fn pass(x: D) -> m::Data { x }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let declaration = snapshot
        .definition_at(root, text.find("m::Data").unwrap())
        .unwrap();
    assert_eq!(declaration.location.file, types);
    assert_eq!(declaration.name, "Data");
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(signature.params[0].ty, signature.return_type);
}

#[test]
fn transitive_type_visibility_changes_invalidate_signatures_and_old_targets_remain() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let facade = insert(&mut db, "facade", "pub use pkg::types::Data;");
    let text = "use pkg::facade::Data; fn pass(x: Data) -> Data { x } fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = AnalysisDatabase::default();
    let first = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let location = text.find("Data)").unwrap();
    let old = first.definition_at(root, location).unwrap().clone();
    db.set(
        "mem://types",
        "struct Data { val value: i32 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        first.file(facade).unwrap().source().revision(),
        second.file(facade).unwrap().source().revision()
    );
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap().signatures(),
        second.file(root).unwrap().signatures()
    ));
    assert!(second.definition_at(root, location).is_none());
    assert_eq!(first.definition_at(root, location), Some(&old));
    let file = second.file(root).unwrap();
    assert!(
        file.result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. }))
    );
    assert_eq!(
        file.type_at(text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn body_edit_cannot_reuse_signatures_after_transitive_type_change() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    insert(&mut db, "facade", "pub use pkg::types::Data;");
    let text = "use pkg::facade::Data; fn pass(x: Data) -> Data { x } fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = AnalysisDatabase::default();
    analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    db.set("mem://root", text.replace("42", "43"), SourceLayer::Overlay)
        .unwrap();
    let edited = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(edited.file(root).unwrap().signatures_reused());
    db.set(
        "mem://types",
        "struct Data { val value: i32 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    db.set("mem://root", text.replace("42", "44"), SourceLayer::Overlay)
        .unwrap();
    let changed = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let file = changed.file(root).unwrap();
    assert!(!file.signatures_reused());
    let fresh = analyze(&db);
    assert_eq!(
        file.signatures().facts().functions(),
        fresh.file(root).unwrap().signatures().facts().functions()
    );
    assert_eq!(
        file.result().diagnostics(),
        fresh.file(root).unwrap().result().diagnostics()
    );
    assert!(
        file.signatures()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. }))
    );
}

#[test]
fn generic_parameters_shadow_type_imports_and_missing_annotations_keep_other_facts() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    let text = "use pkg::types::Data as T; fn keep<T>(x: T) -> T { x } fn number() -> i32 { keep(42) } fn broken(x: Absent, y: T) -> T { y }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert_eq!(
        file.result().diagnostics().len(),
        1,
        "{:?}",
        file.result().diagnostics()
    );
    assert_eq!(
        file.type_at(text.find("keep(42)").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let broken = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "broken")
        .unwrap();
    assert_eq!(broken.params[0].ty, TypeId::Error);
    assert!(matches!(broken.params[1].ty, TypeId::Struct(_)));
    assert_eq!(broken.params[1].ty, broken.return_type);
}
