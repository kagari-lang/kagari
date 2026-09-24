use super::*;
use crate::analysis::{AnalysisDatabase, AnalysisSnapshot};
use kagari_common::{
    identity::PackageId,
    source_database::{SourceDatabase, SourceLayer},
};

fn identity(name: &str) -> ModuleIdentity {
    ModuleIdentity {
        package: PackageId("pkg".into()),
        path: name.split("::").map(str::to_owned).collect(),
    }
}
pub(super) fn insert(db: &mut SourceDatabase, name: &str, text: &str) -> FileId {
    let path = format!("mem://{name}");
    db.bind_module(&path, identity(name)).unwrap();
    db.set(&path, text.into(), SourceLayer::Base).unwrap()
}
pub(super) fn analyze(db: &SourceDatabase) -> AnalysisSnapshot {
    AnalysisDatabase::default()
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn diamond_has_deterministic_dependency_first_order() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "root",
        "use pkg::right; use pkg::left; fn good() -> i32 { 42 }",
    );
    insert(&mut db, "right", "use pkg::shared;");
    insert(&mut db, "left", "use pkg::shared;");
    insert(&mut db, "shared", "pub fn value() -> i32 { 7 }");
    insert(&mut db, "unrelated", "use pkg::unrelated;");
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph
            .initialization_order(&identity("root"), &Default::default())
            .unwrap(),
        ["shared", "left", "right", "root"].map(identity)
    );
    assert!(matches!(
        graph.initialization_order(&identity("unrelated"), &Default::default()),
        Err(ModuleOrderError::Cycle(_))
    ));
    let root = graph.node(&identity("root")).unwrap();
    assert!(
        snapshot
            .file(root.file)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert!(
        snapshot
            .check_program(root.file, &Default::default())
            .is_ok()
    );
}

#[test]
fn cycles_are_reported_on_cycle_edges_without_mislabeling_dependents() {
    let mut db = SourceDatabase::default();
    let a = insert(&mut db, "a", "use pkg::b; fn good() -> i32 { 42 }");
    insert(&mut db, "b", "use pkg::a;");
    let caller = insert(&mut db, "caller", "use pkg::a;");
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph.initialization_order(&identity("caller"), &Default::default()),
        Err(ModuleOrderError::Cycle(vec![identity("a"), identity("b")]))
    );
    assert!(
        !snapshot
            .file(caller)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::CyclicImport { .. }))
    );
    let diagnostics = snapshot.file(a).unwrap().result().diagnostics();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::CyclicImport { .. }))
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .file(a)
            .unwrap()
            .type_at("use pkg::b; fn good() -> i32 { ".len()),
        Some(crate::types::TypeId::Builtin(
            crate::types::BuiltinType::I32
        ))
    );
}

#[test]
fn definition_queries_distinguish_modules_and_follow_source_facades() {
    let mut db = SourceDatabase::default();
    let left = insert(&mut db, "left", "pub fn same() -> i32 { 1 }");
    let right = insert(&mut db, "right", "pub fn same() -> i32 { 2 }");
    insert(&mut db, "facade", "pub use pkg::left::same as exported;");
    let text = "use pkg::left as l; use pkg::right as r; use pkg::facade::exported; fn query() { l::same(); r::same(); exported(); }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    let a = snapshot
        .definition_at(root, text.find("l::same()").unwrap() + "l::".len())
        .unwrap();
    let b = snapshot
        .definition_at(root, text.find("r::same()").unwrap() + "r::".len())
        .unwrap();
    assert_eq!(a.location.file, left);
    assert_eq!(b.location.file, right);
    assert_ne!(a.id, b.id);
    assert_eq!(
        snapshot
            .definition_at(root, text.rfind("exported()").unwrap())
            .unwrap()
            .id,
        a.id
    );
    assert_eq!(
        snapshot
            .source_import_at(root, text.find("pkg::right").unwrap())
            .unwrap()
            .module,
        identity("right")
    );
}

#[test]
fn dependency_overlay_invalidates_cached_imports_and_preserves_old_snapshot() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "library", "pub fn value() -> i32 { 1 }");
    let text = "use pkg::library::value; fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = AnalysisDatabase::default();
    let first = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let old = first
        .definition_at(root, text.find("pkg::library").unwrap())
        .unwrap()
        .clone();
    let cached = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        first.file(root).unwrap(),
        cached.file(root).unwrap()
    ));
    db.set(
        "mem://library",
        "pub fn value() -> i32 { 2 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap(),
        second.file(root).unwrap()
    ));
    assert_eq!(
        second
            .file(root)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies,
        0
    );
    assert_eq!(
        first
            .definition_at(root, text.find("pkg::library").unwrap())
            .unwrap(),
        &old
    );
    assert_ne!(
        second
            .definition_at(root, text.find("pkg::library").unwrap())
            .unwrap()
            .location
            .revision,
        old.location.revision
    );
    db.set(
        "mem://library",
        "fn value() -> i32 { 2 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let private = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(
        private
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ImportNotPublic { .. }))
    );
    assert!(matches!(
        private
            .module_graph()
            .initialization_order(&identity("root"), &Default::default()),
        Err(ModuleOrderError::InvalidImports(_))
    ));
    db.close_overlay("mem://library").unwrap();
    let restored = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(
        restored
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
}

#[test]
fn adding_and_removing_an_overlay_module_reanalyzes_unchanged_importers() {
    let mut db = SourceDatabase::default();
    let text = "use pkg::editor::value; fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = AnalysisDatabase::default();
    let missing = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(
        !missing
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    db.bind_module("mem://editor", identity("editor")).unwrap();
    let editor = db
        .set(
            "mem://editor",
            "pub fn value() -> i32 { 1 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let supplied = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(
        supplied
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let offset = text.find("pkg::editor").unwrap();
    assert_eq!(
        supplied.definition_at(root, offset).unwrap().location.file,
        editor
    );
    assert_eq!(
        missing.file(root).unwrap().source().revision(),
        supplied.file(root).unwrap().source().revision()
    );
    assert!(!Arc::ptr_eq(
        missing.file(root).unwrap(),
        supplied.file(root).unwrap()
    ));
    db.close_overlay("mem://editor").unwrap();
    let removed = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(removed.file(editor).is_none());
    assert!(removed.definition_at(root, offset).is_none());
    assert!(
        !removed
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert_eq!(
        supplied.definition_at(root, offset).unwrap().location.file,
        editor
    );
}

#[test]
fn source_host_and_module_item_ambiguities_are_rejected() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "api", "pub fn child() -> i32 { 1 }");
    insert(&mut db, "api::child", "");
    let root = insert(&mut db, "root", "use pkg::api; use pkg::api::child;");
    let mut analysis = AnalysisDatabase::default();
    analysis.set_host_declarations(
        HostDeclarations::new(kagari_common::host_interface::HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![kagari_common::host_interface::HostFunctionDeclaration::new(
                "pkg.api.external",
                vec![],
                kagari_common::host_interface::HostValueType::Unit,
            )],
        })
        .unwrap(),
    );
    let snapshot = analysis
        .snapshot(db.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::AmbiguousImport { .. }))
            .count(),
        2
    );
}

#[test]
fn graph_traversal_is_cancellable_and_uses_an_explicit_stack() {
    let mut db = SourceDatabase::default();
    for i in 0..1024 {
        insert(
            &mut db,
            &format!("m{i}"),
            &if i == 1023 {
                String::new()
            } else {
                format!("use pkg::m{};", i + 1)
            },
        );
    }
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph
            .initialization_order(&identity("m0"), &Default::default())
            .unwrap()
            .len(),
        1024
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        graph.initialization_order(&identity("m0"), &cancel),
        Err(ModuleOrderError::Cancelled)
    );
}
