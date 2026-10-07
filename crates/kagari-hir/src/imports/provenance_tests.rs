use crate::{
    host::HostDeclarations,
    imports::{
        BindingOrigin, DirectiveResolution, ImportKind, LocalName, ModuleGraph, ModuleOrderError,
        NamespaceId, ResolvedTarget, SourceUnit,
        catalog::{LookupContext, LookupResult, NamespaceResult},
        tests::{analyze, insert},
    },
    lower::lower_module,
    resolver::table::NameResolution,
};
use kagari_source::{source::SourceFile, source_database::SourceDatabase};
use kagari_types::declaration::names::NameNamespace;
use std::sync::Arc;

#[test]
fn grouped_directives_keep_leaf_root_ranges_and_explicit_aliases() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "m", "pub mod nested;");
    let leaf = insert(
        &mut sources,
        "m::nested",
        "pub fn value() -> i32 { 7 } pub struct Data {}",
    );
    let text = "use pkg::m::{nested::value as v, nested::Data}; use pkg::m; fn main(x: m::nested::Data) -> i32 { m::nested::value() + v() }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    assert!(
        snapshot.check_program(root, &Default::default()).is_ok(),
        "{:?}",
        snapshot.file(root).unwrap().result().diagnostics()
    );
    let imports = &snapshot.file(root).unwrap().result().facts().names.imports;
    assert_eq!(imports.directives.len(), 3);
    let [first, second, third] = imports.directives.as_slice() else {
        unreachable!()
    };
    assert!(matches!(&first.kind, ImportKind::Named { alias: Some(name) } if name.as_str() == "v"));
    assert!(matches!(second.kind, ImportKind::Named { alias: None }));
    assert!(matches!(third.kind, ImportKind::Named { alias: None }));
    assert_eq!(
        &text[first.span.range.start..first.span.range.end],
        "nested::value as v"
    );
    assert_eq!(
        &text[second.span.range.start..second.span.range.end],
        "nested::Data"
    );
    assert_eq!(first.root_span, second.root_span);
    assert_eq!(
        &text[first.root_span.range.start..first.root_span.range.end],
        "pkg::m::{nested::value as v, nested::Data}"
    );
    for path in ["m::nested::Data", "m::nested::value()"] {
        let offset = text.rfind(path).unwrap();
        let root_hit = snapshot.source_target_at(root, offset).unwrap();
        assert!(
            matches!(root_hit.target, ResolvedTarget::Namespace(NamespaceId::Module(ref unit)) if unit.module.path == ["m"])
        );
        let child = snapshot
            .source_target_at(root, offset + "m::".len())
            .unwrap();
        assert!(
            matches!(child.target, ResolvedTarget::Namespace(NamespaceId::Module(ref unit)) if unit.module.path == ["m", "nested"])
        );
        let target = snapshot
            .source_target_at(root, offset + "m::nested::".len())
            .unwrap();
        assert!(
            matches!(target.target, ResolvedTarget::Source(ref source) if source.unit.file == leaf)
        );
        assert!(matches!(
            target.via.first(),
            Some(BindingOrigin::NamedImport(_))
        ));
    }
    let grouped_root = text.find("pkg::m").unwrap() + "pkg::".len();
    assert!(
        snapshot
            .module_documentation_at(root, grouped_root)
            .is_some()
    );
}

#[test]
fn equal_glob_targets_keep_both_origins_and_strong_errors_block_fallback() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "library", "pub fn value() -> i32 { 7 }");
    insert(&mut sources, "left", "pub use pkg::library::value;");
    insert(&mut sources, "right", "pub use pkg::library::value;");
    let text = "use pkg::left::*; use pkg::right::*; fn main() -> i32 { value() }";
    let root = insert(&mut sources, "root", text);
    let broken_text = "use pkg::left::missing as value; use pkg::right::*; use missing::Option; fn main() -> i32 { value() }";
    let broken = insert(&mut sources, "broken", broken_text);
    let snapshot = analyze(&sources);
    assert!(snapshot.check_program(root, &Default::default()).is_ok());
    let hit = snapshot
        .source_target_at(root, text.rfind("value()").unwrap())
        .unwrap();
    assert_eq!(
        hit.via
            .iter()
            .filter(|origin| matches!(origin, BindingOrigin::GlobImport(_)))
            .count(),
        2
    );
    let imports = &snapshot
        .file(broken)
        .unwrap()
        .result()
        .facts()
        .names
        .imports;
    assert_eq!(
        imports.scope.lookup("value", NameNamespace::Value),
        Some(NameResolution::Unresolved)
    );
    assert_eq!(
        imports.scope.lookup("Option", NameNamespace::Type),
        Some(NameResolution::Unresolved)
    );
    assert!(matches!(
        imports.directives[0].resolution,
        DirectiveResolution::Named(_)
    ));
    assert!(
        imports
            .dependencies
            .iter()
            .any(|module| module.path == ["left"])
    );
    assert!(
        snapshot
            .source_target_at(broken, broken_text.rfind("value()").unwrap())
            .is_none()
    );
}

#[test]
fn unused_leaf_imports_keep_facade_and_implementation_dependencies() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "library", "pub fn value() -> i32 { 1 }");
    insert(&mut sources, "facade", "pub use pkg::library::value;");
    let root = insert(&mut sources, "root", "use pkg::facade::value; fn main() {}");
    let snapshot = analyze(&sources);
    let imports = &snapshot.file(root).unwrap().result().facts().names.imports;
    for name in ["facade", "library"] {
        assert!(
            imports.directives[0]
                .direct_dependencies
                .iter()
                .any(|module| module.path == [name])
        );
        assert!(
            imports
                .dependencies
                .iter()
                .any(|module| module.path == [name])
        );
    }
    assert!(snapshot.check_program(root, &Default::default()).is_ok());
}

#[test]
fn duplicate_logical_modules_keep_source_units_and_reject_absolute_lookup() {
    let left = lower_module(&SourceFile::new(
        "same",
        "fn left() -> i32 { 1 } use missing::one;",
    ));
    let right = lower_module(&SourceFile::new(
        "same",
        "fn right() -> i32 { 2 } use missing::two;",
    ));
    let hosts = HostDeclarations::empty();
    let graph = ModuleGraph::build([&left, &right], &hosts, &Default::default()).unwrap();
    for (lowered, own, other) in [(&left, "left", "right"), (&right, "right", "left")] {
        let unit = SourceUnit::of(lowered);
        let facts = graph.imports_for(&unit).unwrap();
        assert_eq!(facts.directives[0].id.unit, unit);
        assert!(facts.scope.lookup(own, NameNamespace::Value).is_some());
        assert!(facts.scope.lookup(other, NameNamespace::Value).is_none());
        let table = &graph.catalog.namespaces[&NamespaceId::Module(unit)];
        assert!(Arc::ptr_eq(&facts.scope, &table.names));
        assert!(facts.diagnostics.iter().any(|d| matches!(
            d.kind,
            kagari_source::diagnostic::DiagnosticKind::DuplicateDeclaration { .. }
        )));
    }
    assert!(matches!(
        graph.reachable_order(left.source.module_identity(), &Default::default()),
        Err(ModuleOrderError::InvalidImports(_))
    ));
    let ctx = LookupContext {
        importer: left.source.module_identity(),
        hosts: &hosts,
    };
    let path = format!("{}::left", left.source.module_identity());
    assert!(matches!(
        graph
            .catalog
            .absolute(&ctx, &path, NameNamespace::Type, &Default::default())
            .unwrap(),
        LookupResult::Ambiguous(_)
    ));
}

#[test]
fn relowered_same_revision_targets_are_stale_and_strong_collisions_do_not_filter_to_a_winner() {
    let source = SourceFile::new(
        "same",
        "pub struct Data {} pub fn value() -> i32 { 1 } fn value() -> i32 { 2 }",
    );
    let first = lower_module(&source);
    let second = lower_module(&source);
    let hosts = HostDeclarations::empty();
    let old = ModuleGraph::build([&first], &hosts, &Default::default()).unwrap();
    let new = ModuleGraph::build([&second], &hosts, &Default::default()).unwrap();
    let ctx = LookupContext {
        importer: &kagari_common::identity::ModuleIdentity::single_file("other"),
        hosts: &hosts,
    };
    let ns = NamespaceId::Module(SourceUnit::of(&first));
    let LookupResult::Found(hit) = old
        .catalog
        .lookup_member(&ctx, &ns, "Data", NameNamespace::Type, &Default::default())
        .unwrap()
    else {
        panic!("public type")
    };
    assert_eq!(
        new.catalog
            .namespace_of(&ctx, &hit.target, &Default::default())
            .unwrap(),
        NamespaceResult::StaleSource
    );
    assert_eq!(
        new.catalog
            .lookup_member(&ctx, &ns, "Data", NameNamespace::Type, &Default::default())
            .unwrap(),
        LookupResult::StaleSource
    );
    assert!(matches!(
        old.catalog
            .lookup_member(
                &ctx,
                &ns,
                "value",
                NameNamespace::Value,
                &Default::default()
            )
            .unwrap(),
        LookupResult::Ambiguous(_)
    ));
    let facts = old.imports_for(&SourceUnit::of(&first)).unwrap();
    assert!(
        facts
            .scope
            .entries
            .contains_key(&LocalName::new("value").unwrap())
    );
}
