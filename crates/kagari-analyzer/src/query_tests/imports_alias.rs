use crate::query_tests::imports_tests::{analyze, insert};
use crate::{AnalyzerQueries, query_tests::test_analysis};
// Transparent aliases share runtime types while retaining source declaration identity.

use kagari_hir::types::TypeId;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn type_alias_definition_scope_imports_reexports_and_navigation() {
    let mut sources = SourceDatabase::default();
    let library = insert(
        &mut sources,
        "library",
        "pub struct Model { pub val value: i32 } pub type Item = Model; pub type Values<T> = [T];",
    );
    insert(
        &mut sources,
        "facade",
        "pub use pkg::library::{Item as Renamed, Values};",
    );
    let text = "use pkg::facade::{Renamed, Values}; struct Model { val wrong: bool } fn build() -> Renamed { Renamed { value: 42 } } fn pass(x: Values<i32>) -> [i32] { x }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    let target = snapshot
        .queries()
        .definition_at(root, text.find("-> Renamed").unwrap() + 3)
        .unwrap();
    assert_eq!(target.name, "Item");
    assert_eq!(target.location.file, library);
    let signature = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|function| function.name == "build")
        .unwrap();
    let TypeId::Struct(nominal) = &signature.return_type else {
        panic!("normalized struct");
    };
    assert_eq!(
        file.definitions()
            .resolve(nominal.declaration)
            .unwrap()
            .to_path()
            .module
            .path,
        ["library"]
    );
}

#[test]
fn type_alias_navigation_and_template_survive_body_edit_snapshot_reuse() {
    let text = "fn before() {} type Alias<T> = [T]; fn pass(x: Alias<i32>) -> [i32] { x }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("alias-reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut database = test_analysis();
    let old = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_file = old.file(id).unwrap();
    assert!(
        old_file.result().diagnostics().is_empty(),
        "{:?}",
        old_file.result().diagnostics()
    );
    let target = old_file
        .queries()
        .definition_at(text.find("Alias<i32>").unwrap())
        .unwrap();
    let binder = old_file
        .queries()
        .definition_at(text.find("[T]").unwrap() + 1)
        .unwrap();
    let edited = text.replace("fn before() {}", "fn before() { val n = 42; }");
    sources
        .set("alias-reuse.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let new = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let file = new.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    assert!(file.signatures_reused());
    assert_eq!(
        file.queries()
            .definition_at(edited.find("Alias<i32>").unwrap())
            .unwrap()
            .id,
        target.id
    );
    assert_eq!(
        file.queries()
            .definition_at(edited.find("[T]").unwrap() + 1)
            .unwrap()
            .id,
        binder.id
    );
}
