use crate::imports::tests::{analyze, insert};
use crate::tests::test_analysis;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn module_facade_bindings_share_source_type_call_and_navigation_targets() {
    let mut db = SourceDatabase::default();
    let library = insert(
        &mut db,
        "library",
        "pub struct Data { pub val value: i32 } pub fn answer() -> i32 { 7 } pub fn Data() -> i32 { 3 }",
    );
    insert(
        &mut db,
        "facade",
        "pub use pkg::library as api; pub use pkg::library::*; pub use alloc::vec::Vec::new; pub use alloc::vec;",
    );
    let text = "use pkg::facade::{api as lib, Data as Item}; use pkg::facade; fn main() -> lib::Data { val lib = 0; val Option = 1; val option: Option<i32> = None; Item { value: { val a: Vec<i32> = facade::new(); val b: Vec<i32> = facade::vec::Vec::new(); lib::answer() + Item() + lib } } }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    for offset in [text.find("Data as").unwrap(), text.find("Item").unwrap()] {
        let targets = snapshot.source_targets_at(root, offset);
        assert_eq!(targets.len(), 2, "{targets:?}");
        assert_ne!(targets[0].namespace, targets[1].namespace);
        assert_ne!(targets[0].target, targets[1].target);
        assert!(snapshot.source_target_at(root, offset).is_none());
        assert!(snapshot.definition_at(root, offset).is_none());
    }
    let type_target = snapshot
        .definition_at(root, text.find("lib::Data").unwrap() + 5)
        .unwrap();
    let value_target = snapshot
        .definition_at(root, text.find("Item()").unwrap())
        .unwrap();
    assert_ne!(type_target.id, value_target.id);
    let file = snapshot.file(root).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    snapshot.check_program(root, &Default::default()).unwrap();
    for spelling in ["lib::Data", "lib::answer()"] {
        let target = snapshot
            .definition_at(
                root,
                text.find(spelling).unwrap()
                    + if spelling.starts_with("lib::") {
                        "lib::".len()
                    } else {
                        0
                    },
            )
            .unwrap();
        assert_eq!(target.source_location().unwrap().file, library);
    }
    assert_eq!(
        file.source_function_at(text.find("lib::answer()").unwrap() + "lib::".len())
            .unwrap()
            .id
            .unit
            .file,
        library
    );
}
*/

#[test]
fn body_edit_cannot_reuse_signatures_after_transitive_type_change() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "types", "pub struct Data { val value: i32 }");
    insert(&mut db, "facade", "pub use pkg::types::Data;");
    let text = "use pkg::facade::Data; fn pass(x: Data) -> Data { x } fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = test_analysis();
    analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    db.set("mem://root", text.replace("42", "43"), SourceLayer::Overlay)
        .unwrap();
    let edited = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    // The baseline snapshot was dropped; weak graph caches cannot supply old IDs.
    assert!(!edited.file(root).unwrap().signatures_reused());
    db.set("mem://root", text.replace("42", "44"), SourceLayer::Overlay)
        .unwrap();
    let retained_edit = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert!(retained_edit.file(root).unwrap().signatures_reused());
    db.set(
        "mem://types",
        "struct Data { val value: i32 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    db.set("mem://root", text.replace("42", "45"), SourceLayer::Overlay)
        .unwrap();
    let changed = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let file = changed.file(root).unwrap();
    assert!(!file.signatures_reused());
    let fresh = analyze(&db);
    file.signatures().as_ref().facts().assert_same_source_facts(
        file.definitions(),
        fresh.file(root).unwrap().signatures().as_ref().facts(),
        fresh.file(root).unwrap().definitions(),
        file.result().facts().lowered.module.body.arena(),
        fresh
            .file(root)
            .unwrap()
            .result()
            .facts()
            .lowered
            .module
            .body
            .arena(),
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
