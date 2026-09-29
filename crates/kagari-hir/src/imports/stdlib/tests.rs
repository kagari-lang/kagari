use crate::{
    hir::ExportItem,
    host::HostDeclarations,
    imports::{ImportTarget, ModuleGraph, ModuleImports, SourceImport},
    lower::lower_module,
    native::stdlib::InstalledStdlib,
    resolver::ResolvedName,
};
use kagari_common::{
    DiagnosticKind, SourceFile,
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

fn prepare_imports(text: &str) -> Arc<ModuleImports> {
    let installed = InstalledStdlib::prepare(Default::default(), &Default::default()).unwrap();
    let user = lower_module(&SourceFile::new("imports.kgr", text));
    let graph = ModuleGraph::build(
        installed.modules.iter().map(Arc::as_ref).chain([&user]),
        &HostDeclarations::empty(),
        &Default::default(),
    )
    .unwrap();
    graph
        .node(user.source.module_identity())
        .unwrap()
        .imports
        .clone()
}

fn source(imports: &ModuleImports, name: &str, member: Option<&str>) -> SourceImport {
    let index = imports
        .entries
        .iter()
        .position(|entry| entry.alias == name && !entry.glob_root)
        .unwrap();
    let key = match member {
        Some(member) => imports
            .resolve_member(index, member, &HostDeclarations::empty())
            .unwrap(),
        None => ResolvedName::SourceImport(index),
    };
    let ImportTarget::Source(target) = imports.binding(key).unwrap() else {
        panic!("installed declaration must be an ordinary source target");
    };
    target.clone()
}

#[test]
fn standard_namespace_aliases_and_globs_share_the_source_target() {
    let imports = prepare_imports(
        "use std::math::abs as magnitude; use std::math as numbers; use std as library; use std::math::*;",
    );
    assert!(imports.diagnostics.is_empty(), "{:?}", imports.diagnostics);
    let expected = source(&imports, "magnitude", None);
    assert_eq!(expected.module.package.0, "kagari-std");
    assert_eq!(expected.module.path, ["math"]);
    assert!(matches!(expected.item, Some(ExportItem::Function(_))));
    for (name, member) in [
        ("abs", None),
        ("numbers", Some("abs")),
        ("library", Some("math::abs")),
        ("std", Some("math::abs")),
    ] {
        assert_eq!(source(&imports, name, member), expected);
    }
    let installed_type = prepare_imports("use std::array::ArrayList; use std::cmp::Eq;");
    assert!(installed_type.diagnostics.is_empty());
    assert!(matches!(
        source(&installed_type, "ArrayList", None).item,
        Some(ExportItem::OpaqueType(_))
    ));
    assert!(matches!(
        source(&installed_type, "Eq", None).item,
        Some(ExportItem::Trait(_))
    ));
}

#[test]
fn explicit_and_local_std_names_shadow_the_installed_namespace() {
    let explicit = prepare_imports("use std::math as std;");
    assert!(explicit.diagnostics.is_empty());
    assert_eq!(source(&explicit, "std", None).module.path, ["math"]);
    for text in ["fn std() {}", "struct std {}", "const std: i32 = 1;"] {
        let imports = prepare_imports(text);
        assert!(imports.diagnostics.is_empty());
        assert!(imports.entries.iter().all(|entry| entry.alias != "std"));
    }
    let ambiguous = prepare_imports("use std::math as std; use std::debug as std;");
    assert!(
        ambiguous
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::DuplicateImport { .. }))
    );
    assert!(
        ambiguous
            .entries
            .iter()
            .filter(|entry| entry.alias == "std")
            .all(|entry| entry.target.is_none())
    );
}

#[test]
fn declared_prelude_types_and_traits_use_ordinary_import_precedence() {
    let defaults = prepare_imports("");
    assert!(defaults.diagnostics.is_empty());
    for name in ["ArrayList", "String", "Range"] {
        assert!(matches!(
            source(&defaults, name, None).item,
            Some(ExportItem::OpaqueType(_))
        ));
    }
    for name in ["Eq", "Iterable", "From", "Fn"] {
        assert!(matches!(
            source(&defaults, name, None).item,
            Some(ExportItem::Trait(_))
        ));
    }
    assert!(matches!(
        source(&defaults, "Option", None).item,
        Some(ExportItem::Enum(_))
    ));
    let local = prepare_imports("struct Option {}");
    assert!(local.entries.iter().all(|entry| entry.alias != "Option"));
    let explicit = prepare_imports("use std::result::Result as Option;");
    assert_eq!(source(&explicit, "Option", None).module.path, ["result"]);
    let glob = prepare_imports("use std::option::*;");
    assert_eq!(
        glob.entries
            .iter()
            .filter(|entry| !entry.glob_root && entry.alias == "Option")
            .count(),
        1
    );
    assert_eq!(source(&glob, "Option", None).module.path, ["option"]);
}

#[test]
fn copying_installed_names_and_uris_does_not_install_the_package_alias() {
    let mut sources = SourceDatabase::default();
    let uri = "kagari://std/std.kgr";
    sources
        .bind_module(
            uri,
            ModuleIdentity {
                package: PackageId("kagari-std".into()),
                path: vec!["std".into()],
            },
        )
        .unwrap();
    let id = sources
        .set(
            uri,
            "pub fn fake() {} use std::math;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let snapshot = sources.snapshot();
    let lowered = lower_module(snapshot.file(id).unwrap());
    let graph =
        ModuleGraph::build([&lowered], &HostDeclarations::empty(), &Default::default()).unwrap();
    let imports = &graph
        .node(lowered.source.module_identity())
        .unwrap()
        .imports;
    assert!(
        imports
            .diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::UnknownName { .. }))
    );
    assert!(imports.entries.iter().all(|entry| entry.alias != "std"));
}
