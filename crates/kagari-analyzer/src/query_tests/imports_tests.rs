use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::*;
use kagari_hir::imports::*;
use kagari_source::identity::*;
use std::sync::*;

use kagari_common::identity::PackageId;
use kagari_hir::analysis::AnalysisSnapshot;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::declaration::names::NameNamespace;

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
    test_analysis()
        .snapshot(db.snapshot(), &Default::default())
        .unwrap()
}

fn expected_modules(names: &[&str]) -> Vec<ModuleIdentity> {
    let mut expected = names.iter().map(|name| identity(name)).collect::<Vec<_>>();
    expected.extend(
        foundation_catalog::shared()
            .unwrap()
            .into_iter()
            .map(|module| module.records().identity.clone()),
    );
    expected.sort();
    expected
}

#[test]
fn inline_module_queries_use_physical_offsets_and_stable_child_identity() {
    let mut db = SourceDatabase::default();
    let text = "// 中文😀\r\nmod child {
    use super::*; pub fn value() -> i32 { 42 } pub fn call() -> i32 { value() } }";
    let root = insert(&mut db, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let child = identity("root::child");
    let child_id = first
        .module_graph()
        .node(
            &first
                .module_graph()
                .catalog
                .definitions()
                .lookup_module(&child)
                .unwrap(),
        )
        .unwrap()
        .file;
    let reference = text.rfind("value()").unwrap();
    let target = first.queries().definition_at(root, reference).unwrap();
    assert_eq!(target.source_location().unwrap().file, root);
    assert_eq!(
        target.source_location().unwrap().range.start,
        text.find("value()").unwrap()
    );
    db.set("mem://root", text.replace("42", "41"), SourceLayer::Overlay)
        .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        second
            .module_graph()
            .node(
                &second
                    .module_graph()
                    .catalog
                    .definitions()
                    .lookup_module(&child)
                    .unwrap()
            )
            .unwrap()
            .file,
        child_id
    );
    assert_eq!(
        first.file(child_id).unwrap().source().text().find("42"),
        text.find("42")
    );
    assert_eq!(
        second.file(child_id).unwrap().source().text().find("41"),
        text.find("42")
    );
}

#[test]
fn cycles_are_reachable_without_invalidating_dependents() {
    let mut db = SourceDatabase::default();
    let a = insert(&mut db, "a", "use pkg::b; fn good() -> i32 { 42 }");
    insert(&mut db, "b", "use pkg::a;");
    let caller = insert(&mut db, "caller", "use pkg::a;");
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph
            .reachable_order(
                &graph
                    .catalog
                    .definitions()
                    .lookup_module(&identity("caller"))
                    .unwrap(),
                &Default::default()
            )
            .unwrap()
            .iter()
            .map(|module| graph
                .catalog
                .definitions()
                .module_identity(*module)
                .unwrap()
                .clone())
            .collect::<Vec<_>>(),
        expected_modules(&["a", "b", "caller"])
    );
    assert!(
        snapshot
            .file(caller)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    let diagnostics = snapshot.file(a).unwrap().result().diagnostics();
    assert!(diagnostics.is_empty());
    assert_eq!(
        snapshot
            .file(a)
            .unwrap()
            .queries()
            .type_at("use pkg::b; fn good() -> i32 { ".len()),
        Some(kagari_hir::types::TypeId::Builtin(
            kagari_types::scalar::BuiltinType::I32
        ))
    );

    let mut seeded = SourceDatabase::default();
    insert(
        &mut seeded,
        "a",
        "pub fn value() -> i32 { 42 } pub use pkg::b::*;",
    );
    insert(&mut seeded, "b", "pub use pkg::a::*;");
    let caller = insert(
        &mut seeded,
        "caller",
        "use pkg::b::value; fn main() -> i32 { value() }",
    );
    insert(&mut seeded, "dual_a", "pub use pkg::dual_b::Root as N;");
    insert(
        &mut seeded,
        "dual_b",
        "pub use pkg::a as Root; pub use pkg::dual_a::N::value as Root;",
    );
    let dual_caller = insert(
        &mut seeded,
        "dual_caller",
        "use pkg::dual_a::N; fn main() -> i32 { N() + N::value() }",
    );
    let solved = analyze(&seeded);
    solved.check_program(caller, &Default::default()).unwrap();
    let queries = solved.queries();
    let offset = "use pkg::dual_a::".len();
    let hits = queries.source_targets_at(dual_caller, offset);
    assert_eq!(hits.len(), 2);
    assert!(queries.source_target_at(dual_caller, offset).is_none());
    let types = hits
        .iter()
        .find(|hit| hit.namespace == NameNamespace::Type)
        .unwrap();
    let values = hits
        .iter()
        .find(|hit| hit.namespace == NameNamespace::Value)
        .unwrap();
    assert!(
        matches!(&types.target, ResolvedTarget::Namespace(NamespaceId::Module(unit))
        if solved.module_graph().catalog.definitions().module_identity(unit.module).unwrap() == &identity("a"))
    );
    assert!(matches!(&values.target, ResolvedTarget::Source(source)
        if solved.module_graph().catalog.definitions().module_identity(source.unit.module).unwrap() == &identity("a")
            && matches!(source.item, SourceItem::Function(_))));
    for hit in &hits {
        assert!(
            matches!(hit.via.first(), Some(BindingOrigin::NamedImport(id)) if id.unit.file == dual_caller)
        );
    }
    assert!(
        solved
            .check_program(dual_caller, &Default::default())
            .is_ok(),
        "{:?}",
        solved.file(dual_caller).unwrap().result().diagnostics()
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
        .queries()
        .definition_at(root, text.find("l::same()").unwrap() + "l::".len())
        .unwrap();
    let b = snapshot
        .queries()
        .definition_at(root, text.find("r::same()").unwrap() + "r::".len())
        .unwrap();
    assert_eq!(a.source_location().unwrap().file, left);
    assert_eq!(b.source_location().unwrap().file, right);
    assert_ne!(a.id, b.id);
    assert_eq!(
        snapshot
            .queries()
            .definition_at(root, text.rfind("exported()").unwrap())
            .unwrap()
            .id,
        a.id
    );
    assert_eq!(
        snapshot
            .queries()
            .source_target_at(root, text.find("pkg::right").unwrap())
            .unwrap()
            .target,
        ResolvedTarget::Namespace(NamespaceId::Module(
            snapshot
                .file(right)
                .unwrap()
                .result()
                .facts()
                .names
                .items
                .source_unit()
                .copied()
                .unwrap()
        ))
    );
}

#[test]
fn dependency_overlay_invalidates_cached_imports_and_preserves_old_snapshot() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "library", "pub fn value() -> i32 { 1 }");
    let text = "use pkg::library::value; fn good() -> i32 { 42 }";
    let root = insert(&mut db, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let old = first
        .queries()
        .definition_at(root, text.find("pkg::library").unwrap())
        .unwrap()
        .clone();
    let cached = analysis
        .snapshot(db.snapshot(), &Default::default())
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
        .snapshot(db.snapshot(), &Default::default())
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
            .reused_bodies(),
        0
    );
    assert_eq!(
        first
            .queries()
            .definition_at(root, text.find("pkg::library").unwrap())
            .unwrap(),
        &old
    );
    assert_ne!(
        second
            .queries()
            .definition_at(root, text.find("pkg::library").unwrap())
            .unwrap()
            .source_location()
            .unwrap()
            .revision,
        old.source_location().unwrap().revision
    );
    db.set(
        "mem://library",
        "fn value() -> i32 { 2 }".into(),
        SourceLayer::Overlay,
    )
    .unwrap();
    let private = analysis
        .snapshot(db.snapshot(), &Default::default())
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
        private.module_graph().reachable_order(
            &private
                .module_graph()
                .catalog
                .definitions()
                .lookup_module(&identity("root"))
                .unwrap(),
            &Default::default()
        ),
        Err(ModuleOrderError::InvalidImports(_))
    ));
    db.close_overlay("mem://library").unwrap();
    let restored = analysis
        .snapshot(db.snapshot(), &Default::default())
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
    let mut analysis = test_analysis();
    let missing = analysis
        .snapshot(db.snapshot(), &Default::default())
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
        .snapshot(db.snapshot(), &Default::default())
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
        supplied
            .queries()
            .definition_at(root, offset)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
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
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert!(removed.file(editor).is_none());
    assert!(removed.queries().definition_at(root, offset).is_none());
    assert!(
        !removed
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert_eq!(
        supplied
            .queries()
            .definition_at(root, offset)
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        editor
    );
}

#[test]
fn associated_methods_respect_owner_visibility_and_type_aliases() {
    let mut sources = SourceDatabase::default();
    let library = insert(
        &mut sources,
        "library",
        "struct Hidden {} impl Hidden { pub fn make() -> Hidden { Hidden {} } } pub struct Visible {} impl Visible { pub fn make() -> Visible { Visible {} } fn secret() -> i32 { 0 } }",
    );
    let valid = insert(
        &mut sources,
        "valid",
        "use pkg::library::Visible as Item; fn main() -> Item { Item::make() }",
    );
    let invalid = insert(
        &mut sources,
        "invalid",
        "use pkg::library::Hidden::make; use pkg::library::Visible::secret;",
    );
    let snapshot = analyze(&sources);
    let analysis = snapshot.file(valid).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let offset = analysis.source().text().find("make()").unwrap();
    let declaration = snapshot.queries().definition_at(valid, offset).unwrap();
    assert_eq!(declaration.name, "make");
    assert_eq!(declaration.source_location().unwrap().file, library);
    let analysis = snapshot.file(invalid).unwrap();
    assert_eq!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .filter(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::ImportNotPublic { .. }))
            .count(),
        2
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn deep_aliases_and_direct_leaves_share_declarations_without_child_bindings() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "m", "pub mod nested;");
    let leaf = insert(
        &mut db,
        "m::nested",
        "pub fn value() -> i32 { 7 } pub const LIMIT: i32 = 9; pub struct Data {} pub enum Choice { One }",
    );
    insert(&mut db, "other", "pub mod nested;");
    let other = insert(&mut db, "other::nested", "pub fn value() -> i32 { 8 }");
    let text = "use pkg::m as a; use pkg::m as b; use pkg::m::nested::value as v; use pkg::other as c; fn main() -> i32 { a::nested::value() + b::nested::value() + v() + c::nested::value() }";
    let root = insert(&mut db, "root", text);
    let snapshot = analyze(&db);
    assert!(
        snapshot.check_program(root, &Default::default()).is_ok(),
        "{:?}",
        snapshot.file(root).unwrap().result().diagnostics()
    );
    let hits = [
        "a::nested::value()",
        "b::nested::value()",
        "v()",
        "c::nested::value()",
    ]
    .map(|path| {
        snapshot
            .queries()
            .source_target_at(root, text.rfind(path).unwrap() + path.len() - 3)
            .unwrap()
    });
    assert_eq!(hits[0].target, hits[1].target);
    assert_eq!(hits[0].target, hits[2].target);
    assert_ne!(hits[0].via, hits[1].via);
    assert_ne!(hits[0].target, hits[3].target);
    assert_eq!(
        snapshot
            .queries()
            .definition_at(
                root,
                text.find("a::nested::value()").unwrap() + "a::nested::".len()
            )
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        leaf
    );
    assert_eq!(
        snapshot
            .queries()
            .definition_at(
                root,
                text.find("c::nested::value()").unwrap() + "c::nested::".len()
            )
            .unwrap()
            .source_location()
            .unwrap()
            .file,
        other
    );
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .facts()
            .names
            .items
            .lookup("nested", NameNamespace::Type)
            .is_none()
    );
    let missing = insert(
        &mut db,
        "missing",
        "use pkg::m; fn bad() -> i32 { nested::value() }",
    );
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(missing)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::UnknownName { .. }))
    );
}
