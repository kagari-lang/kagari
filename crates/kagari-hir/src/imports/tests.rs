use super::*;
use crate::tests::standard::standard_modules;
use crate::{
    analysis::{
        AnalysisDatabase, AnalysisSnapshot, core_source::packaged_modules, error::AnalysisError,
    },
    lower::lower_module,
    tests::{native_fixture::function_module, test_analysis},
};
use kagari_common::{
    cancellation::CancellationToken,
    identity::{PackageId, map::DefinitionContext},
};
use kagari_source::{
    diagnostic::DiagnosticKind,
    package::{PackageEnvironment, PackageScope},
    source_database::{CapturedSource, SourceDatabase, SourceLayer},
};
use kagari_types::{declaration::names::NameNamespace, scalar::BuiltinType, ty::Ty};

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
        standard_modules()
            .into_iter()
            .map(|module| module.records().identity.clone()),
    );
    expected.extend(
        packaged_modules()
            .iter()
            .map(|input| input.source.module_identity().clone()),
    );
    expected.sort();
    expected
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
    let independent = insert(
        &mut db,
        "independent",
        "use pkg::left::Marker; use pkg::right::*; fn main() -> i32 { val value: Marker = Marker {}; Marker() }",
    );
    let snapshot = analyze(&db);
    snapshot
        .check_program(independent, &Default::default())
        .unwrap();
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
    let snapshot = analyze(&db);
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
    let outsider = insert(
        &mut db,
        "outsider",
        "use pkg::root::family::shared; use pkg::root::family::child;",
    );
    let snapshot = analyze(&db);
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
fn package_visibility_covers_siblings_but_not_other_packages_or_public_reexports() {
    let mut db = SourceDatabase::default();
    insert(
        &mut db,
        "root",
        "pub mod inner { pub(crate) fn value() -> i32 { 7 } pub(crate) struct Data { pub(crate) val value: i32 } } pub(crate) use self::inner::value as shared;",
    );
    let sibling = insert(
        &mut db,
        "sibling",
        "use pkg::root::shared; use pkg::root::inner::Data; fn main() -> i32 { val data = Data { value: shared() }; data.value }",
    );
    let snapshot = analyze(&db);
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
    snapshot
        .check_program(sibling, &Default::default())
        .unwrap();
    let leak = insert(&mut db, "leak", "pub use pkg::root::shared as leaked;");
    let foreign_path = "mem://foreign";
    db.bind_module(
        foreign_path,
        ModuleIdentity {
            package: PackageId("other".into()),
            path: vec![],
        },
    )
    .unwrap();
    let foreign = db
        .set(
            foreign_path,
            "use pkg::root::shared; use pkg::root::inner::Data;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    db.bind_module(
        "mem://package",
        ModuleIdentity {
            package: PackageId("pkg".into()),
            path: vec![],
        },
    )
    .unwrap();
    db.set("mem://package", "pub mod root;".into(), SourceLayer::Base)
        .unwrap();
    let packages = PackageEnvironment::new(
        [
            (PackageId("pkg".into()), PackageScope::default()),
            (
                PackageId("other".into()),
                PackageScope {
                    dependencies: [("pkg".into(), PackageId("pkg".into()))].into(),
                    ..Default::default()
                },
            ),
        ]
        .into(),
    )
    .unwrap();
    let captured = db
        .snapshot()
        .files()
        .map(|file| CapturedSource {
            key: file.name().into(),
            display_name: file.name().into(),
            module: file.module_identity().clone(),
            text: file.text().into(),
        })
        .collect();
    let source = db
        .replace_captured(captured, packages, &Default::default())
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(source, &Default::default())
        .unwrap();
    for (file, expected) in [(foreign, 2), (leak, 1)] {
        let result = snapshot.file(file).unwrap().result();
        assert_eq!(
            result
                .diagnostics()
                .iter()
                .filter(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::ImportNotPublic { .. }
                ))
                .count(),
            expected,
            "{:?}",
            result.diagnostics()
        );
    }
}

#[test]
fn wildcard_import_expands_offline_host_module_declarations() {
    let mut db = SourceDatabase::default();
    let root = insert(
        &mut db,
        "root",
        "use demo::*; fn main() -> i32 { echo(42) }",
    );
    let mut analysis = test_analysis();
    let module = function_module(
        ModuleIdentity {
            package: PackageId("demo".into()),
            path: vec![],
        },
        "echo",
        vec![("value", Ty::Builtin(BuiltinType::I32))],
        Ty::Builtin(BuiltinType::I32),
    );
    let mut modules = standard_modules();
    modules.push(module);
    analysis.set_native_modules(modules);
    let snapshot = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    let imports = &snapshot
        .module_graph()
        .node(
            &snapshot
                .module_graph()
                .catalog
                .definitions
                .lookup_module(&identity("root"))
                .unwrap(),
        )
        .unwrap()
        .imports;
    assert!(matches!(
        imports
            .scope
            .lookup("echo", NameNamespace::Value)
            .and_then(|b| b.target()),
        Some(ResolvedName::Source(_))
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
    let unrelated = db
        .set(
            "mem://unrelated",
            "use self as recursive;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = analyze(&db);
    let graph = snapshot.module_graph();
    assert_eq!(
        graph
            .reachable_order(
                &graph
                    .catalog
                    .definitions
                    .lookup_module(&identity("root"))
                    .unwrap(),
                &Default::default()
            )
            .unwrap()
            .iter()
            .map(|module| graph.catalog.identity(*module).clone())
            .collect::<Vec<_>>(),
        expected_modules(&["left", "right", "root", "shared"])
    );
    assert_eq!(
        graph
            .reachable_order(
                &graph
                    .catalog
                    .definitions
                    .lookup_module(snapshot.file(unrelated).unwrap().source().module_identity())
                    .unwrap(),
                &Default::default()
            )
            .unwrap()
            .iter()
            .map(|module| graph.catalog.identity(*module).clone())
            .collect::<Vec<_>>(),
        {
            let mut expected = expected_modules(&[]);
            expected.push(
                snapshot
                    .file(unrelated)
                    .unwrap()
                    .source()
                    .module_identity()
                    .clone(),
            );
            expected.sort();
            expected
        }
    );
    let root = graph
        .node(
            &snapshot
                .module_graph()
                .catalog
                .definitions
                .lookup_module(&identity("root"))
                .unwrap(),
        )
        .unwrap();
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
fn duplicate_source_and_structured_module_owners_are_rejected() {
    let mut db = SourceDatabase::default();
    insert(&mut db, "api", "pub fn child() -> i32 { 1 }");
    insert(&mut db, "api::child", "");
    let root = insert(&mut db, "root", "use pkg::api; use pkg::api::child;");
    let mut analysis = test_analysis();
    let module = function_module(
        identity("api"),
        "external",
        vec![],
        Ty::Builtin(BuiltinType::Unit),
    );
    let mut modules = standard_modules();
    modules.push(module);
    // Distinct type/value namespace spellings are valid in the original snapshot.
    let snapshot = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap();
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    analysis.set_native_modules(modules);
    let error = analysis
        .snapshot(db.snapshot(), &Default::default())
        .unwrap_err();
    assert!(
        matches!(error, AnalysisError::NativeApi(error) if error.0.contains("source conflicts with library declaration input"))
    );
    // Failed replacement cannot change the retained successful import graph.
    assert!(
        snapshot
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
}

#[test]
fn declared_source_aliases_cannot_conflict_with_supplied_package_bindings() {
    let mut db = SourceDatabase::default();
    let modules = ["app", "library"].map(|name| CapturedSource {
        key: format!("mem://{name}"),
        display_name: name.into(),
        module: ModuleIdentity {
            package: PackageId(name.into()),
            path: vec![],
        },
        text: String::new(),
    });
    let packages = PackageEnvironment::new(
        [
            (
                PackageId("app".into()),
                PackageScope {
                    dependencies: [("std".into(), PackageId("library".into()))].into(),
                    ..Default::default()
                },
            ),
            (PackageId("library".into()), PackageScope::default()),
        ]
        .into(),
    )
    .unwrap();
    let source = db
        .replace_captured(modules.into(), packages, &Default::default())
        .unwrap();
    let error = test_analysis()
        .snapshot(source, &Default::default())
        .unwrap_err();
    assert!(
        matches!(error, AnalysisError::NativeApi(error) if error.0.contains("conflicting package binding std"))
    );

    // A module dependency retains reachability, not authority over an explicit alias.
    let mut sources = SourceDatabase::new("/aliases").unwrap();
    let mut root = None;
    for (package, text) in [
        ("left", "pub fn value() -> i32 { 1 }"),
        ("right", "pub fn value() -> i32 { 2 }"),
        (
            "app",
            "use left::value as imported; fn main() -> i32 { imported() } fn qualified() -> i32 { left::value() }",
        ),
    ] {
        let identity = ModuleIdentity {
            package: PackageId(package.into()),
            path: vec![],
        };
        sources.bind_module(package, identity).unwrap();
        let id = sources
            .set(package, text.into(), SourceLayer::Base)
            .unwrap();
        if package == "app" {
            root = Some(id);
        }
    }
    let root = root.unwrap();
    let mut analysis = AnalysisDatabase::new("/aliases").unwrap();
    analysis.set_import_environment(environment::ImportEnvironment {
        modules: [(
            sources
                .snapshot()
                .file(root)
                .unwrap()
                .module_identity()
                .clone(),
            environment::ModuleScope {
                packages: PackageScope {
                    dependencies: [("left".into(), PackageId("right".into()))].into(),
                    ..Default::default()
                },
                dependencies: ["left", "right"]
                    .map(|name| ModuleIdentity {
                        package: PackageId(name.into()),
                        path: vec![],
                    })
                    .into(),
                preludes: Default::default(),
            },
        )]
        .into(),
    });
    let snapshot = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    snapshot.check_program(root, &Default::default()).unwrap();
    let file = snapshot.file(root).unwrap();
    let facts = file.result().facts();
    let calls = facts
        .lowered
        .module
        .nodes
        .expressions()
        .filter_map(|(id, _)| facts.typed.type_table().call_resolution(id))
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 2);
    for call in calls {
        let crate::typeck::table::CallTarget::SourceFunction(target) = call.target else {
            panic!("source function call")
        };
        assert_eq!(
            file.definitions()
                .resolve(target)
                .unwrap()
                .module()
                .package
                .0,
            "right"
        );
    }
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
    let source = db.snapshot();
    let lowered = source
        .files()
        .map(|file| lower_module(file))
        .collect::<Vec<_>>();
    let graph = ModuleGraph::build(
        &lowered,
        &DefinitionContext::new().unwrap(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        graph
            .reachable_order(
                &graph
                    .catalog
                    .definitions
                    .lookup_module(&identity("m0"))
                    .unwrap(),
                &Default::default()
            )
            .unwrap()
            .len(),
        1024
    );
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert_eq!(
        graph.reachable_order(
            &graph
                .catalog
                .definitions
                .lookup_module(&identity("m0"))
                .unwrap(),
            &cancel
        ),
        Err(ModuleOrderError::Cancelled)
    );
}

mod aggregate;
mod alias;
mod cache;
mod provenance;
mod signature;
mod solver;
mod types;

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
                    .definitions
                    .lookup_module(&identity("caller"))
                    .unwrap(),
                &Default::default()
            )
            .unwrap()
            .iter()
            .map(|module| graph.catalog.identity(*module).clone())
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
    let definitions = DefinitionContext::new().unwrap();
    let forward = ModuleGraph::build(&lowered, &definitions, &Default::default()).unwrap();
    let reverse =
        ModuleGraph::build(lowered.iter().rev(), &definitions, &Default::default()).unwrap();
    assert_eq!(forward.catalog.modules, reverse.catalog.modules);
    assert_eq!(forward.catalog.namespaces, reverse.catalog.namespaces);
    assert_eq!(forward.catalog.packages, reverse.catalog.packages);
    assert_eq!(forward.catalog.environment, reverse.catalog.environment);
    assert_eq!(
        forward.catalog.module_dependencies,
        reverse.catalog.module_dependencies
    );

    assert_eq!(forward.source_facts, reverse.source_facts);
}
