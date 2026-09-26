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
fn imported_struct_initializers_and_nested_mutations_use_nominal_fields() {
    let mut db = SourceDatabase::default();
    let models = insert(
        &mut db,
        "models",
        "pub struct Inner { pub var number: i32 } pub struct Outer { pub val inner: Inner }",
    );
    let text = "use pkg::models as m; struct Inner { var number: bool } fn make() -> m::Outer { m::Outer { inner: m::Inner { number: 42 } } } fn update(x: m::Outer) -> i32 { x.inner.number += 1; x.inner.number }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(models)
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
    for offset in [
        text.find("x.inner.number +=").unwrap(),
        text.rfind("x.inner.number").unwrap(),
    ] {
        let field = snapshot
            .definition_at(root, offset + "x.inner.".len())
            .unwrap();
        assert_eq!(field.location.file, models);
        assert_eq!(field.name, "number");
    }
    for (id, _) in file.result().facts().lowered.module.body.expressions() {
        if let Some(init) = file.result().facts().typed.type_table.struct_init(id) {
            assert_eq!(init.structure.module.path, ["models"]);
            for field in init.fields.iter().flatten() {
                assert_eq!(field.module.path, ["models"]);
                assert_eq!(
                    file.result().facts().aggregates.field(field).unwrap().slot,
                    0
                );
            }
        }
    }
    assert!(file.result().clone().into_codegen().is_ok());
}

#[test]
fn private_foreign_fields_are_rejected_but_public_fields_remain_accessible() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "models",
        "pub struct Data { val hidden: i32, pub val shown: i32 }",
    );
    let root = insert(
        &mut db,
        "root",
        "use pkg::models::Data; fn read(x: Data) -> i32 { x.shown + x.hidden }",
    );
    let snapshot = analyze(&db);
    let diagnostics = snapshot.file(root).unwrap().result().diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownName { .. })),
        "{diagnostics:?}"
    );
}

#[test]
fn imported_inherent_method_navigation_uses_its_source_declaration() {
    let mut db = SourceDatabase::default();
    let model = insert(
        &mut db,
        "model",
        "pub struct Data { val value: i32 } impl Data { pub fn read(self) -> i32 { self.value } } pub fn make() -> Data { Data { value: 42 } }",
    );
    let text = "use pkg::model::make; fn main() -> i32 { make().read() }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let at = text.find("read()").unwrap();
    assert_eq!(
        snapshot.definition_at(root, at).unwrap().location.file,
        model
    );
    assert_eq!(
        snapshot
            .file(root)
            .unwrap()
            .source_function_at(at)
            .unwrap()
            .id
            .file,
        model
    );
}

#[test]
fn pub_super_field_is_visible_in_parent_tree_and_hidden_outside() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "root",
        "pub mod model { pub struct Data { pub(super) val value: i32 } pub fn make() -> Data { Data { value: 42 } } }",
    );
    let parent = insert(
        &mut db,
        "root::peer",
        "use pkg::root::model::make; fn main() -> i32 { make().value }",
    );
    let outsider = insert(
        &mut db,
        "outsider",
        "use pkg::root::model::make; fn main() -> i32 { make().value }",
    );
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(parent)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert!(
        snapshot
            .file(outsider)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownName { .. }))
    );
}

#[test]
fn incomplete_foreign_member_access_retains_the_nominal_receiver() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "models", "pub struct Data { var count: i32 }");
    let text = "use pkg::models::Data; fn good() -> i32 { 42 } fn partial(x: Data) { x. }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    assert!(!file.result().diagnostics().is_empty());
    assert!(
        matches!(file.member_receiver_type(text.find("x.").unwrap() + 2), Some(TypeId::Struct(id)) if id.declaration.module.path == ["models"])
    );
    assert_eq!(
        file.type_at(text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn imported_readonly_fields_and_initializer_errors_keep_receiver_facts() {
    let mut db = SourceDatabase::default();
    let models = insert(
        &mut db,
        "models",
        "pub struct Data { pub val fixed: i32, pub var count: i32 }",
    );
    let text = "use pkg::models::Data; fn bad(x: Data) { x.fixed = 1; x.count = true; } fn broken() -> Data { Data { count: true, extra: 2 } } fn good(x: Data) -> i32 { x.count }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let file = snapshot.file(root).unwrap();
    let diagnostics = file.result().diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. }))
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::AssignmentTypeMismatch { .. }))
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::InvalidStructInitializer { .. }))
            .count(),
        2
    );
    assert_eq!(
        snapshot
            .definition_at(root, text.find("fixed =").unwrap())
            .unwrap()
            .location
            .file,
        models
    );
    assert_eq!(
        file.type_at(text.rfind("x.count").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn inferred_foreign_fields_invalidate_through_unchanged_function_facades() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "models", "pub struct Data { pub var count: i32 }");
    let api = insert(
        &mut db,
        "api",
        "use pkg::models::Data; pub fn make() -> Data { Data { count: 1 } }",
    );
    let text = "use pkg::api::make; fn read() -> i32 { make().count }";
    let root = insert(&mut db, "root", text);
    let unrelated = insert(
        &mut db,
        "unrelated",
        "struct Data { var count: bool } fn unrelated() -> i32 { 0 }",
    );
    let mut analysis = AnalysisDatabase::default();
    let first = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    db.set(
        "mem://models",
        "pub struct Data { pub var count: bool }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        first.file(api).unwrap().source().revision(),
        second.file(api).unwrap().source().revision()
    );
    assert_eq!(
        first
            .file(root)
            .unwrap()
            .result()
            .facts()
            .imported_functions,
        second
            .file(root)
            .unwrap()
            .result()
            .facts()
            .imported_functions
    );
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap(),
        second.file(root).unwrap()
    ));
    assert!(Arc::ptr_eq(
        first.file(unrelated).unwrap(),
        second.file(unrelated).unwrap()
    ));
    assert!(
        second
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ReturnTypeMismatch { .. }))
    );
    assert_eq!(
        first
            .file(root)
            .unwrap()
            .type_at(text.find("make().count").unwrap() + "make().".len()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        second
            .file(root)
            .unwrap()
            .type_at(text.find("make().count").unwrap() + "make().".len()),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
}
