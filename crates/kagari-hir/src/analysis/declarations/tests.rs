use super::*;
use crate::{
    analysis::AnalysisSnapshot,
    declarations::DeclarationId,
    native::{NativeBinding, NativeTypeKind, render::declaration_source},
    resolver::resolved::ResolvedName,
    tests::test_analysis,
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_stdlib::catalog as foundation_catalog;
use kagari_syntax::parser;
use kagari_types::declaration::names::NameNamespace;

fn query(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> DeclarationSnapshot {
    db.declarations(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn native_cache_retains_preparation_but_not_graphs_across_user_revisions() {
    let mut sources = SourceDatabase::default();
    sources
        .set("main.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = query(&mut db, &sources);
    let file = first
        .files()
        .find(|file| {
            file.source().module_identity() == &kagari_stdlib::namespaces::module("core", "iter")
        })
        .unwrap();
    assert!(
        file.declarations()
            .iter()
            .any(|item| item.name == "Iterator")
    );
    let id = file.source().id();
    let source = file.declared.lowered.source.clone();
    let location = file.syntax().location();
    let arena = file.declared.lowered.module.body.arena();
    let method_count = file.declared.lowered.method_policies.len();
    assert!(method_count > 0);
    let old_type = file.declared.lowered.module.functions[0].params[0].ty;
    let graph_handles = first
        .files()
        .map(|file| Arc::downgrade(&file.declared.lowered))
        .collect::<Vec<_>>();
    sources
        .set(
            "main.kgr",
            "fn main() { val changed = 1; }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = query(&mut db, &sources);
    assert!(Arc::ptr_eq(
        first.file(id).unwrap(),
        second.file(id).unwrap()
    ));
    let mut fork = db.clone();
    drop(first);
    drop(second);
    assert!(
        graph_handles
            .iter()
            .all(|handle| handle.upgrade().is_none())
    );
    assert!(db.native_files.get().is_some());

    // A locator resolves only against the original physical parse, not a reparse.
    let third = query(&mut db, &sources);
    let file = third.file(id).unwrap();
    assert!(location.resolve(file.parsed.physical()).is_some());
    assert!(Arc::ptr_eq(&source, &file.declared.lowered.source));
    let lowered = &file.declared.lowered;
    assert_ne!(lowered.module.body.arena(), arena);
    assert!(lowered.registered_native_api && lowered.language_foundation);
    assert!(!lowered.registered_traits.is_empty());
    assert_eq!(lowered.method_policies.len(), method_count);
    for method in &lowered.module.traits[0].methods {
        assert!(lowered.method_policies.contains_key(&method.function));
    }
    assert_ne!(lowered.module.functions[0].params[0].ty, old_type);
    let adapter = lowered
        .module
        .opaque_types
        .iter()
        .find(|item| item.name == "MapIterator")
        .unwrap();
    assert!(matches!(
        lowered.native_types.get(&adapter.id),
        Some(NativeTypeKind::Storage { arity: 2, .. })
    ));
    let iterable = lowered
        .module
        .traits
        .iter()
        .find(|item| item.name == "Iterable")
        .unwrap();
    let map = iterable
        .methods
        .iter()
        .find(|item| item.name == "map")
        .unwrap();
    assert!(map.has_default);
    assert!(matches!(
        lowered.native_functions.get(&map.function),
        Some(NativeBinding::Default(_))
    ));
    assert!(!lowered.module.impls.is_empty());
    for method in lowered.module.impls.iter().flat_map(|item| &item.methods) {
        assert!(matches!(
            lowered.native_functions.get(&method.function),
            Some(NativeBinding::Entry(_))
        ));
    }

    fork.set_parse_limits(parser::ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    assert!(fork.native_files.get().is_none());
    let limited = query(&mut fork, &sources);
    let replacement = limited
        .files()
        .find(|file| file.source().module_identity() == source.module_identity())
        .unwrap();
    assert!(location.resolve(replacement.parsed.physical()).is_none());
    assert!(location.resolve(file.parsed.physical()).is_some());
    drop(db);
    drop(fork);
    assert_eq!(file.source().text(), source.text());
    assert!(
        file.declarations()
            .iter()
            .any(|item| item.name == "Iterator")
    );
}

#[test]
fn cancelled_or_invalid_native_import_never_publishes_a_snapshot() {
    let sources = SourceDatabase::default();
    let mut db = test_analysis();
    let cancelled = CancellationToken::default();
    cancelled.cancel();
    assert!(matches!(
        db.declarations(sources.snapshot(), &cancelled),
        Err(AnalysisError::Cancelled)
    ));
    assert!(db.native_files.get().is_none());
    assert!(db.declaration_cache.is_none());
    db.set_parse_limits(parser::ParseLimits {
        max_tree_depth: 1,
        ..Default::default()
    });
    assert!(matches!(
        db.declarations(sources.snapshot(), &Default::default()),
        Err(AnalysisError::NativeApi(_))
    ));
    assert!(db.native_files.get().is_none());
    assert!(db.declaration_cache.is_none());
    db.set_parse_limits(Default::default());
    let modules = db.native_modules.clone();
    assert!(modules.len() > 1);
    let mut views = modules
        .iter()
        .map(|module| declaration_source(module, &modules).unwrap())
        .collect::<Vec<_>>();
    // Fail the last source after earlier providers have prepared successfully.
    views
        .last_mut()
        .unwrap()
        .text
        .push_str("\npub trait Broken {");
    db.set_native_sources(modules.clone(), views).unwrap();
    let error = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap_err();
    assert!(
        matches!(error, AnalysisError::NativeApi(error) if error.0.contains("invalid native declaration syntax"))
    );
    assert!(db.native_files.get().is_none());
    assert!(db.declaration_cache.is_none());
    db.set_native_modules(modules);
    let complete = query(&mut db, &sources);
    assert_eq!(complete.files.len(), db.native_files.get().unwrap().len());
}

#[test]
fn declaration_query_stops_before_body_resolution_signatures_and_const_evaluation() {
    let mut sources = SourceDatabase::default();
    let text = "struct Point { var x: i32 } trait Show { fn show(self) -> i32; } fn bad<T: Absent>(value: Missing) -> Missing { unknown } const BAD: i32 = 1 / 0; fn good(value: i32) -> i32 { val local = value; local }";
    let id = sources
        .set("headers.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let declarations = query(&mut db, &sources);
    let file = declarations.file(id).unwrap();
    assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    for name in ["Point", "x", "Show", "show", "bad", "T", "BAD", "good"] {
        assert!(
            file.declarations().iter().any(|d| d.name == name),
            "missing {name}"
        );
    }
    assert!(
        file.declarations()
            .iter()
            .all(|d| !matches!(d.id, DeclarationId::Binding(_)))
    );
    assert!(
        file.names()
            .items
            .lookup("good", NameNamespace::Value)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Function(_))))
    );
    assert!(
        db.files.is_empty(),
        "declarations must not populate body results"
    );
    let complete = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        file,
        complete.declaration_snapshot().file(id).unwrap()
    ));
    let full = complete.file(id).unwrap();
    assert!(!full.result().diagnostics().is_empty());
    assert!(!full.signatures().diagnostics().is_empty());
    for declaration in file.declarations().iter() {
        assert_eq!(complete.declaration(&declaration.id), Some(declaration));
    }
    let local = full.definition_at(text.rfind("local }").unwrap()).unwrap();
    assert!(matches!(local.id, DeclarationId::Binding(_)));
    assert!(declarations.declaration(&local.id).is_none());
    let again = query(&mut db, &sources);
    assert!(Arc::ptr_eq(file, again.file(id).unwrap()));
    assert!(again.declaration(&local.id).is_none());
}

#[test]
fn declaration_cache_tracks_dependencies_and_keeps_unrelated_files_shared() {
    let mut sources = SourceDatabase::default();
    let mut insert = |name: &str, text: &str| {
        let path = format!("mem://{name}");
        sources
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        sources.set(&path, text.into(), SourceLayer::Base).unwrap()
    };
    let dependency = insert("dep", "pub struct Data { val value: i32 }");
    let root = insert("root", "use pkg::dep::Data; fn pass(x: Data) -> Data { x }");
    let unrelated = insert("other", "fn other() -> i32 { 42 }");
    sources
        .bind_module(
            "mem://other",
            ModuleIdentity {
                package: PackageId("independent".into()),
                path: vec!["other".into()],
            },
        )
        .unwrap();
    let mut db = test_analysis();
    let first = query(&mut db, &sources);
    assert!(first.file(root).unwrap().diagnostics().is_empty());
    let old = first
        .file(dependency)
        .unwrap()
        .declarations()
        .iter()
        .find(|d| d.name == "Data")
        .unwrap()
        .clone();
    sources
        .set(
            "mem://dep",
            "// 中文😀\r\nstruct Data { val value: i32 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = query(&mut db, &sources);
    assert!(!Arc::ptr_eq(
        first.file(root).unwrap(),
        second.file(root).unwrap()
    ));
    assert!(Arc::ptr_eq(
        first.file(unrelated).unwrap(),
        second.file(unrelated).unwrap()
    ));
    assert!(
        second
            .file(root)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ImportNotPublic { .. }))
    );
    assert_eq!(first.declaration(&old.id), Some(&old));
    assert_ne!(second.declaration(&old.id).unwrap().location, old.location);
    let fresh = query(&mut test_analysis(), &sources);
    assert_eq!(
        second.file(root).unwrap().diagnostics(),
        fresh.file(root).unwrap().diagnostics()
    );
    sources.close_overlay("mem://dep").unwrap();
    let restored = query(&mut db, &sources);
    assert!(restored.file(root).unwrap().diagnostics().is_empty());
    assert!(db.files.is_empty());
}

#[test]
fn declaration_queries_keep_recovery_and_reject_stale_cache_publication() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "recovery.kgr",
            "fn old() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = test_analysis();
    let old_source = sources.snapshot();
    let old = query(&mut db, &sources);
    sources
        .set(
            "recovery.kgr",
            "fn good() -> i32 { 42 } fn bad() { val x = ; }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let latest = query(&mut db, &sources);
    assert!(
        latest
            .file(id)
            .unwrap()
            .names()
            .items
            .lookup("good", NameNamespace::Value)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Function(_))))
    );
    assert!(!latest.file(id).unwrap().diagnostics().is_empty());
    assert!(old.file(id).unwrap().diagnostics().is_empty());
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(db.declarations(old_source.clone(), &cancel).is_err());
    let stale = db.declarations(old_source, &Default::default()).unwrap();
    assert!(
        stale
            .file(id)
            .unwrap()
            .names()
            .items
            .lookup("old", NameNamespace::Value)
            .is_some_and(|r| matches!(r.target(), Some(ResolvedName::Function(_))))
    );
    let again = query(&mut db, &sources);
    assert!(Arc::ptr_eq(
        latest.file(id).unwrap(),
        again.file(id).unwrap()
    ));
    assert_eq!(latest.revision(), sources.snapshot().revision());
    assert!(db.files.is_empty());
}

#[test]
fn changing_parser_budget_invalidates_queries_without_changing_old_snapshots() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "budget.kgr",
            "fn good() -> i32 { 42 } @ @ fn later() -> i32 { 7 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = test_analysis();
    let old = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let has_later = |snapshot: &AnalysisSnapshot| {
        snapshot
            .file(id)
            .unwrap()
            .result()
            .facts()
            .declarations
            .iter()
            .any(|d| d.name == "later")
    };
    assert!(has_later(&old));
    let mut fork = db.clone();
    fork.set_parse_limits(kagari_syntax::parser::ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    let limited = fork
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!has_later(&limited));
    assert!(has_later(&old));
    assert!(limited.check_program(id, &Default::default()).is_err());
    assert!(
        limited
            .file(id)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(
                d.kind,
                DiagnosticKind::CompileLimitExceeded {
                    resource: "parser diagnostics",
                    limit: 0
                }
            ))
    );
    let unchanged = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(has_later(&unchanged));
    fork.set_parse_limits(Default::default());
    let restored = fork
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(has_later(&restored));
    assert!(!has_later(&limited));
    assert!(
        restored
            .file(id)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .all(|d| !matches!(d.kind, DiagnosticKind::CompileLimitExceeded { .. }))
    );
}

#[test]
fn declaration_records_outlive_queries_without_owning_the_lowering() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "types.kgr",
            include_str!("../../../tests/fixtures/analysis/records/types.kgr").into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = query(&mut db, &sources);
    let file = first.file(id).unwrap();
    assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    let declaration = file
        .declarations()
        .iter()
        .find(|declaration| declaration.name == "Data")
        .unwrap()
        .clone();
    let record = file.declared.record.clone();
    let lowered = file.declared.lowered.clone();
    let arena = lowered.module.body.arena();
    let query_handle = Arc::downgrade(file);
    let graph_handle = Arc::downgrade(&lowered);
    let fork = db.clone();
    drop(first);
    assert!(query_handle.upgrade().is_none());
    assert!(graph_handle.upgrade().is_some());
    assert!(Arc::ptr_eq(
        &record,
        &db.declaration_cache.as_ref().unwrap().files[&id].record
    ));

    // Another live graph owner permits rebuilding a view without cloning facts.
    let second = query(&mut db, &sources);
    let file = second.file(id).unwrap();
    assert!(Arc::ptr_eq(&record, &file.declared.record));
    assert!(Arc::ptr_eq(&lowered, &file.declared.lowered));
    assert_eq!(second.declaration(&declaration.id), Some(&declaration));
    drop(second);
    drop(lowered);
    assert!(graph_handle.upgrade().is_none());
    assert!(
        db.declaration_cache.as_ref().unwrap().files[&id]
            .query
            .upgrade()
            .is_none()
    );
    assert!(Arc::ptr_eq(
        &record,
        &fork.declaration_cache.as_ref().unwrap().files[&id].record
    ));

    // The owned IDs survive as evidence, not as permission to reuse a dead graph.
    let third = query(&mut db, &sources);
    let file = third.file(id).unwrap();
    assert_ne!(file.declared.lowered.module.body.arena(), arena);
    assert!(!Arc::ptr_eq(&record, &file.declared.record));
    assert_eq!(third.declaration(&declaration.id), Some(&declaration));
    assert!(graph_handle.upgrade().is_none());
}

#[test]
fn default_analysis_has_no_implicit_native_catalog() {
    let mut sources = SourceDatabase::default();
    sources
        .set(
            "main.kgr",
            "fn value(x: Option<i32>) -> Option<i32> { x }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut database = AnalysisDatabase::default();
    let root = sources.snapshot().files().next().unwrap().id();
    let declarations = query(&mut database, &sources);
    assert_eq!(declarations.files().count(), 1);
    let analyzed = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        !analyzed
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    database.set_native_modules(foundation_catalog::shared().unwrap());
    let installed = database
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        installed
            .file(root)
            .unwrap()
            .result()
            .diagnostics()
            .is_empty()
    );
    assert_eq!(declarations.files().count(), 1);
}
