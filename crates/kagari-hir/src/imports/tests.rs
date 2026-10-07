use super::*;
use crate::{analysis::AnalysisSnapshot, tests::test_analysis};
use crate::{host::HostDeclarations, lower::lower_module};
use kagari_common::cancellation::CancellationToken;
use kagari_common::identity::PackageId;
use kagari_source::diagnostic::DiagnosticKind;
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::{declaration::names::NameNamespace, host_interface::value_type::HostValueType};

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
            .into_iter()
            .map(|module| module.identity.clone()),
    );
    expected.sort();
    expected
}

#[test]
fn inline_module_queries_use_physical_offsets_and_stable_child_identity() {
    let mut db = SourceDatabase::default();
    let text =
        "// 中文😀\r\nmod child { pub fn value() -> i32 { 42 } pub fn call() -> i32 { value() } }";
    let root = insert(&mut db, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let child = identity("root::child");
    let child_id = first.module_graph().node(&child).unwrap().file;
    let reference = text.rfind("value()").unwrap();
    let target = first.definition_at(root, reference).unwrap();
    assert_eq!(target.location.file, root);
    assert_eq!(target.location.range.start, text.find("value()").unwrap());
    db.set("mem://root", text.replace("42", "41"), SourceLayer::Overlay)
        .unwrap();
    let second = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(second.module_graph().node(&child).unwrap().file, child_id);
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
fn wildcard_imports_detect_conflicts_and_reject_nonmodule_targets() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "left",
        "pub struct Marker {} pub fn same() -> i32 { 1 }",
    );
    insert(
        &mut db,
        "right",
        "pub fn Marker() -> i32 { 42 } pub fn same() -> i32 { 2 }",
    );
    let conflict = insert(
        &mut db,
        "conflict",
        "use pkg::left::*; use pkg::right::*; fn main() -> i32 { same() }",
    );
    let invalid = insert(
        &mut db,
        "invalid",
        "use pkg::left::same::*; fn main() -> i32 { 42 }",
    );
    let independent = insert(
        &mut db,
        "independent",
        "use pkg::left::Marker; use pkg::right::*; fn main() -> i32 { val value: Marker = Marker {}; Marker() }",
    );
    let snapshot = analyze(&db);
    snapshot
        .check_program(independent, &Default::default())
        .unwrap();
    assert!(
        snapshot
            .file(conflict)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::DuplicateImport { .. }))
    );
    assert!(
        snapshot
            .file(invalid)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::InvalidGlobTarget { .. }))
    );
}

#[test]
fn private_inline_children_are_not_importable_from_other_modules() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "root",
        "mod child { pub fn value() -> i32 { 42 } }",
    );
    let outsider = insert(
        &mut db,
        "outsider",
        "use pkg::root::child; fn main() -> i32 { child::value() }",
    );
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(outsider)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ImportNotPublic { .. }))
    );
}

#[test]
fn scoped_visibility_allows_parent_tree_and_rejects_outsiders() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "root",
        "pub mod family { pub(super) fn shared() -> i32 { 7 } fn hidden() -> i32 { 8 } pub(super) mod child { pub fn value() -> i32 { 9 } } }",
    );
    let parent = insert(
        &mut db,
        "root::caller",
        "use pkg::root::family::shared; use pkg::root::family::child::value; fn main() -> i32 { shared() + value() }",
    );
    let sibling = insert(
        &mut db,
        "root::family::sibling",
        "use pkg::root::family::hidden; fn main() -> i32 { hidden() }",
    );
    let outsider = insert(
        &mut db,
        "outsider",
        "use pkg::root::family::shared; use pkg::root::family::child;",
    );
    let snapshot = analyze(&db);
    assert!(
        snapshot
            .file(parent)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(parent).unwrap().result().diagnostics()
    );
    assert!(
        snapshot
            .file(sibling)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(sibling).unwrap().result().diagnostics()
    );
    snapshot.check_program(parent, &Default::default()).unwrap();
    snapshot
        .check_program(sibling, &Default::default())
        .unwrap();
    assert!(
        snapshot
            .file(outsider)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .filter(|d| matches!(d.kind, DiagnosticKind::ImportNotPublic { .. }))
            .count()
            >= 2
    );
}

#[test]
fn reexports_cannot_widen_private_items_or_modules() {
    let mut db = SourceDatabase::default();
    let facade = insert(
        &mut db,
        "facade",
        "mod hidden { pub fn value() -> i32 { 1 } pub struct secret {} fn secret() -> i32 { 2 } } pub use self::hidden::value; pub use self::hidden::secret; pub use self::hidden as leaked;",
    );
    let outsider = insert(
        &mut db,
        "outsider",
        "use pkg::facade::value; fn main() -> i32 { value() }",
    );
    let snapshot = analyze(&db);
    let errors = snapshot
        .file(facade)
        .unwrap()
        .result()
        .diagnostics()
        .iter()
        .filter(|d| matches!(d.kind, DiagnosticKind::ImportNotPublic { .. }))
        .count();
    assert_eq!(
        errors,
        2,
        "{:?}",
        snapshot.file(facade).unwrap().result().diagnostics()
    );
    assert!(
        snapshot
            .file(outsider)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(outsider).unwrap().result().diagnostics()
    );
}

#[test]
fn wildcard_import_expands_offline_host_module_declarations() {
    use kagari_types::host_interface::{
        HostFunctionDeclaration, HostInterface, HostParameter, HostPassingStyle,
        value_type::HostValueType,
    };
    let mut db = SourceDatabase::default();
    let root = insert(
        &mut db,
        "root",
        "use demo::*; fn main() -> i32 { echo(42) }",
    );
    let mut analysis = test_analysis();
    analysis.set_host_declarations(
        HostDeclarations::new(HostInterface {
            paths: vec![],
            types: vec![],
            functions: vec![HostFunctionDeclaration::new(
                "demo.echo",
                vec![HostParameter {
                    name: "value".into(),
                    ty: HostValueType::I32,
                    passing: HostPassingStyle::Owned,
                }],
                HostValueType::I32,
            )],
        })
        .unwrap(),
    );
    let snapshot = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let imports = &snapshot
        .module_graph()
        .node(&identity("root"))
        .unwrap()
        .imports;
    assert!(matches!(
        imports
            .scope
            .lookup("echo", NameNamespace::Value)
            .and_then(|b| b.target()),
        Some(ResolvedName::HostFunction(_))
    ));
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty(),
        "{:?}",
        snapshot.file(root).unwrap().result().diagnostics()
    );
}

#[test]
fn diamond_has_deterministic_reachable_order() {
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
            .reachable_order(&identity("root"), &Default::default())
            .unwrap(),
        expected_modules(&["left", "right", "root", "shared"])
    );
    assert_eq!(
        graph
            .reachable_order(&identity("unrelated"), &Default::default())
            .unwrap(),
        expected_modules(&["unrelated"])
    );
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
fn cycles_are_reachable_without_invalidating_dependents() {
    let mut db = SourceDatabase::default();
    let a = insert(&mut db, "a", "use pkg::b; fn good() -> i32 { 42 }");
    insert(&mut db, "b", "use pkg::a;");
    let caller = insert(&mut db, "caller", "use pkg::a;");
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph
            .reachable_order(&identity("caller"), &Default::default())
            .unwrap(),
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
            .type_at("use pkg::b; fn good() -> i32 { ".len()),
        Some(crate::types::TypeId::Builtin(
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
    assert!(
        solved
            .check_program(dual_caller, &Default::default())
            .is_ok(),
        "{:?}",
        solved.file(dual_caller).unwrap().result().diagnostics()
    );
    let input = seeded.snapshot();
    let lowered = input
        .files()
        .map(|file| lower_module(file))
        .collect::<Vec<_>>();
    let forward =
        ModuleGraph::build(&lowered, &HostDeclarations::empty(), &Default::default()).unwrap();
    let reverse = ModuleGraph::build(
        lowered.iter().rev(),
        &HostDeclarations::empty(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(forward.catalog, reverse.catalog);
    assert_eq!(forward.source_facts, reverse.source_facts);
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
            .source_target_at(root, text.find("pkg::right").unwrap())
            .unwrap()
            .target,
        ResolvedTarget::Namespace(NamespaceId::Module(SourceUnit::of(
            &snapshot.file(right).unwrap().result().facts().lowered
        )))
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
        private
            .module_graph()
            .reachable_order(&identity("root"), &Default::default()),
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
        .snapshot(db.snapshot(), &Default::default())
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
    let mut analysis = test_analysis();
    analysis.set_host_declarations(
        HostDeclarations::new(kagari_types::host_interface::HostInterface {
            paths: vec![],
            types: Vec::new(),
            functions: vec![kagari_types::host_interface::HostFunctionDeclaration::new(
                "pkg.api.external",
                vec![],
                HostValueType::Unit,
            )],
        })
        .unwrap(),
    );
    let snapshot = analysis
        .snapshot(db.snapshot(), &Default::default())
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
        1
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
            .reachable_order(&identity("m0"), &Default::default())
            .unwrap()
            .len(),
        1024 + foundation_catalog::shared().len()
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        graph.reachable_order(&identity("m0"), &cancel),
        Err(ModuleOrderError::Cancelled)
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
    let declaration = snapshot.definition_at(valid, offset).unwrap();
    assert_eq!(declaration.name, "make");
    assert_eq!(declaration.location.file, library);
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
    let missing = insert(
        &mut db,
        "missing",
        "use pkg::m; fn bad() -> i32 { nested::value() }",
    );
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
            .source_target_at(root, text.rfind(path).unwrap() + path.len() - 3)
            .unwrap()
    });
    assert_eq!(hits[0].target, hits[1].target);
    assert_eq!(hits[0].target, hits[2].target);
    assert_ne!(hits[0].via, hits[1].via);
    assert_ne!(hits[0].target, hits[3].target);
    assert_eq!(
        snapshot
            .definition_at(
                root,
                text.find("a::nested::value()").unwrap() + "a::nested::".len()
            )
            .unwrap()
            .location
            .file,
        leaf
    );
    assert_eq!(
        snapshot
            .definition_at(
                root,
                text.find("c::nested::value()").unwrap() + "c::nested::".len()
            )
            .unwrap()
            .location
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
