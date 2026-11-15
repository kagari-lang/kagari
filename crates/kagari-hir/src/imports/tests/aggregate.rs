use crate::imports::tests::{analyze, insert};
use kagari_source::{diagnostic::DiagnosticKind, source_database::SourceDatabase};

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
fn struct_update_requires_every_foreign_field_to_be_accessible() {
    for fields in ["..base", "shown: 2, ..base", "shown: 2, hidden: 3, ..base"] {
        let mut sources = SourceDatabase::default();
        let models = insert(
            &mut sources,
            "models",
            "pub struct Data { val hidden: i32, pub val shown: i32 } pub fn local(base: Data) -> Data { Data { shown: 1, ..base } }",
        );
        let root = insert(
            &mut sources,
            "root",
            &format!(
                "use pkg::models::Data; fn main(base: Data) -> Data {{ Data {{ {fields} }} }}"
            ),
        );
        let snapshot = analyze(&sources);
        let analysis = snapshot.file(root).unwrap();
        snapshot
            .file(models)
            .unwrap()
            .result()
            .clone()
            .into_codegen()
            .expect("same-module private field access");
        assert!(analysis.result().diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::InvalidStructInitializer { reason, .. } if reason == "field `hidden` is not accessible for struct update"
        )), "{:?}", analysis.result().diagnostics());
        assert!(analysis.result().clone().into_codegen().is_err());
    }
    let mut sources = SourceDatabase::default();
    let models = insert(
        &mut sources,
        "models",
        "pub struct Data { pub val first: i32, pub val second: i32 } pub fn local(base: Data) -> Data { Data { first: 1, ..base } }",
    );
    let root = insert(
        &mut sources,
        "root",
        "use pkg::models::Data; fn main(base: Data) -> Data { Data { first: 1, ..base } }",
    );
    let snapshot = analyze(&sources);
    for file in [models, root] {
        snapshot
            .file(file)
            .unwrap()
            .result()
            .clone()
            .into_codegen()
            .expect("public foreign and same-module fields");
    }
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
