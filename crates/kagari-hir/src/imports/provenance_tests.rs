use crate::{
    imports::{
        BindingOrigin, DirectiveResolution, ImportKind, NamespaceId, ResolvedTarget,
        tests::{analyze, insert},
    },
    resolver::table::NameResolution,
};
use kagari_source::source_database::SourceDatabase;

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
        imports.scope.lookup("value"),
        Some(NameResolution::Unresolved)
    );
    assert_eq!(
        imports.scope.lookup("Option"),
        Some(NameResolution::Unresolved)
    );
    assert!(matches!(
        imports.directives[0].resolution,
        DirectiveResolution::Unresolved
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
