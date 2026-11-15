use crate::AnalyzerQueries;
use crate::query_tests::imports_tests::{analyze, insert};
use kagari_hir::{
    imports::{
        BindingOrigin, DirectiveResolution, ImportKind, NamespaceId, ResolvedTarget,
        bindings::LookupOutcome,
    },
    lower::lower_module,
    resolver::table::NameResolution,
};
use kagari_source::{
    diagnostic::DiagnosticKind, source::SourceFile, source_database::SourceDatabase,
};
use kagari_types::declaration::names::NameNamespace;

#[test]
fn grouped_directives_keep_leaf_root_ranges_and_explicit_aliases() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "m", "pub mod nested;");
    let leaf = insert(
        &mut sources,
        "m::nested",
        "pub fn value() -> i32 { 7 } pub struct Data {}",
    );
    let text = "// 中文😀\r\nuse pkg::m::{nested::value as v, nested::Data};\r\nuse pkg::m; fn main(x: m::nested::Data) -> i32 { m::nested::value() + v() }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    let queries = snapshot.queries();
    assert!(queries.navigation.borrow().is_empty());
    let cancel = kagari_common::cancellation::CancellationToken::default();
    cancel.cancel();
    assert!(
        crate::navigation::NavigationIndex::build(snapshot.file(root).unwrap(), &cancel).is_err()
    );
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
        let root_hit = queries.source_target_at(root, offset).unwrap();
        assert!(
            matches!(root_hit.target, ResolvedTarget::Namespace(NamespaceId::Module(ref unit)) if snapshot.module_graph().catalog.definitions().module_identity(unit.module).unwrap().path == ["m"])
        );
        let child = snapshot
            .queries()
            .source_target_at(root, offset + "m::".len())
            .unwrap();
        assert!(
            matches!(child.target, ResolvedTarget::Namespace(NamespaceId::Module(ref unit)) if snapshot.module_graph().catalog.definitions().module_identity(unit.module).unwrap().path == ["m", "nested"])
        );
        let target = snapshot
            .queries()
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
    let lowered = lower_module(&SourceFile::new("root", text));
    for (slot, tokens) in [
        vec!["pkg", "m", "nested", "value"],
        vec!["pkg", "m", "nested", "Data"],
        vec!["pkg", "m"],
    ]
    .into_iter()
    .enumerate()
    {
        let sites = lowered.source_map.import_path(slot);
        assert_eq!(sites.len(), tokens.len());
        for ((_, span), token) in sites.iter().zip(tokens) {
            assert_eq!(&text[span.start..span.end], token);
            let hit = queries.source_target_at(root, span.start);
            if token == "pkg" && slot < 2 {
                // Source-only package roots have no namespace target or enclosing leaf fallback.
                assert!(hit.is_none());
                continue;
            }
            let hit = hit.unwrap();
            match token {
                "pkg" => assert!(matches!(hit.target,
                    ResolvedTarget::Namespace(NamespaceId::Module(ref unit))
                        if snapshot.module_graph().catalog.definitions().module_identity(unit.module).unwrap().path == ["m"])),
                "m" | "nested" => assert!(matches!(hit.target,
                    ResolvedTarget::Namespace(NamespaceId::Module(ref unit))
                        if snapshot.module_graph().catalog.definitions().module_identity(unit.module).unwrap().path == if token == "m" { vec!["m"] } else { vec!["m", "nested"] })),
                _ => assert!(matches!(hit.target,
                    ResolvedTarget::Source(ref source) if source.unit.file == leaf)),
            }
        }
    }
    let grouped_root = text.find("pkg::m").unwrap() + "pkg::".len();
    assert!(
        snapshot
            .queries()
            .module_documentation_at(root, grouped_root)
            .is_some()
    );
    let indexes = queries.navigation.borrow();
    assert_eq!(indexes.len(), 1);
    let index = indexes.get(&root).unwrap();
    assert!(std::ptr::eq(
        index.analysis(),
        snapshot.file(root).unwrap().as_ref()
    ));
}

#[test]
fn repeated_super_prefixes_keep_physical_sites_when_root_underflows() {
    let mut sources = SourceDatabase::default();
    let tree = insert(
        &mut sources,
        "tree",
        "pub mod middle; pub fn value() -> i32 { 7 }",
    );
    insert(&mut sources, "tree::middle", "pub mod leaf;");
    let text = "// 中文😀\r\nuse super::super::{value as v, middle::leaf as current};\r\nuse super::super::super::super::value as missing;\r\nfn main() -> i32 { v() }";
    let file = insert(&mut sources, "tree::middle::leaf", text);
    let snapshot = analyze(&sources);
    let facts = &snapshot.file(file).unwrap().result().facts().names.imports;
    assert_eq!(facts.directives.len(), 3);
    assert_eq!(
        facts
            .diagnostics
            .iter()
            .map(|d| d.kind.clone())
            .collect::<Vec<_>>(),
        [DiagnosticKind::UnknownName {
            name: "super::super::super::super::value".into()
        }]
    );
    assert!(snapshot.check_program(file, &Default::default()).is_err());
    let lowered = lower_module(&SourceFile::new("leaf", text));
    for (slot, expected) in [
        vec!["super", "super", "value"],
        vec!["super", "super", "middle", "leaf"],
        vec!["super", "super", "super", "super", "value"],
    ]
    .into_iter()
    .enumerate()
    {
        let sites = lowered.source_map.import_path(slot);
        assert_eq!(sites.len(), expected.len());
        for (index, ((_, span), token)) in sites.iter().zip(expected).enumerate() {
            assert_eq!(&text[span.start..span.end], token);
            let hit = snapshot.queries().source_target_at(file, span.start);
            if slot == 2 && index >= 2 {
                // The third `super` reaches the absent package-root namespace; the fourth underflows.
                assert!(
                    hit.is_none(),
                    "package root/underflow must not resolve at {span:?}"
                );
                continue;
            }
            let hit = hit.unwrap();
            if slot == 0 && index == 2 {
                assert!(matches!(hit.target,
                    ResolvedTarget::Source(ref source) if source.unit.file == tree));
            } else {
                let expected_path = match index {
                    0 | 2 => vec!["tree", "middle"],
                    1 => vec!["tree"],
                    3 => vec!["tree", "middle", "leaf"],
                    _ => unreachable!(),
                };
                assert!(matches!(hit.target,
                    ResolvedTarget::Namespace(NamespaceId::Module(ref unit))
                        if snapshot.module_graph().catalog.definitions().module_identity(unit.module).unwrap().path == expected_path));
            }
        }
    }
    let call = snapshot
        .queries()
        .source_target_at(file, text.rfind("v()").unwrap())
        .unwrap();
    assert!(matches!(call.target, ResolvedTarget::Source(ref source) if source.unit.file == tree));
    assert!(
        matches!(call.via.first(), Some(BindingOrigin::NamedImport(id)) if id == &facts.directives[0].id)
    );
    for namespace in NameNamespace::ALL {
        assert!(facts.scope.lookup("missing", namespace).is_none());
    }
}

#[test]
fn equal_glob_targets_keep_origins_and_absent_categories_release_fallback() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "library", "pub fn value() -> i32 { 7 }");
    insert(&mut sources, "left", "pub use pkg::library::value;");
    insert(&mut sources, "right", "pub use pkg::library::value;");
    let text = "use pkg::left::*; use pkg::right::*; fn main() -> i32 { value() }";
    let root = insert(&mut sources, "root", text);
    let snapshot = analyze(&sources);
    assert!(snapshot.check_program(root, &Default::default()).is_ok());
    let hit = snapshot
        .queries()
        .source_target_at(root, text.rfind("value()").unwrap())
        .unwrap();
    assert_eq!(
        hit.via
            .iter()
            .filter(|origin| matches!(origin, BindingOrigin::GlobImport(_)))
            .count(),
        2
    );
    let broken_text = "use pkg::left::missing as value; use pkg::right::*; use missing::Option; fn main() -> i32 { value() }";
    let broken = insert(&mut sources, "broken", broken_text);
    let snapshot = analyze(&sources);
    let imports = &snapshot
        .file(broken)
        .unwrap()
        .result()
        .facts()
        .names
        .imports;
    assert!(matches!(
        imports.scope.lookup("value", NameNamespace::Value),
        Some(NameResolution::Unique(_))
    ));
    assert!(matches!(
        imports.scope.lookup("Option", NameNamespace::Type),
        Some(NameResolution::Unique(_))
    ));
    assert!(matches!(&imports.directives[0].resolution,
        DirectiveResolution::Named(outcomes) if outcomes.types == LookupOutcome::Absent && outcomes.values == LookupOutcome::Absent));
    assert!(snapshot.check_program(broken, &Default::default()).is_err());
    assert!(imports.dependencies.iter().any(|module| {
        snapshot
            .module_graph()
            .catalog
            .definitions()
            .module_identity(*module)
            .unwrap()
            .path
            == ["left"]
    }));
    assert!(
        snapshot
            .queries()
            .source_target_at(broken, broken_text.rfind("value()").unwrap())
            .is_some()
    );
}
