use super::*;
use crate::{
    analysis::AnalysisSnapshot, declarations::DeclarationId, resolver::resolved::ResolvedName,
};

use {
    kagari_common::identity::{ModuleIdentity, PackageId},
    kagari_source::{
        diagnostic::DiagnosticKind,
        source_database::{SourceDatabase, SourceLayer},
    },
};

fn query(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> DeclarationSnapshot {
    db.declarations(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn native_declarations_are_shared_across_user_revisions_and_retained_by_snapshots() {
    let mut sources = SourceDatabase::default();
    sources
        .set("main.kgr", "fn main() {}".into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = query(&mut db, &sources);
    let module = db.native_files.get().unwrap()[0].1.clone();
    let file = first.file(module.source.id()).unwrap();
    assert!(
        file.declarations()
            .iter()
            .any(|declaration| declaration.name == "ArrayList")
    );
    sources
        .set(
            "main.kgr",
            "fn main() { val changed = 1; }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = query(&mut db, &sources);
    assert!(Arc::ptr_eq(&module, &db.native_files.get().unwrap()[0].1));
    assert!(Arc::ptr_eq(file, second.file(module.source.id()).unwrap()));
    drop(db);
    assert_eq!(
        first.file(module.source.id()).unwrap().source().text(),
        module.source.text()
    );
}

#[test]
fn cancelled_or_invalid_native_import_never_publishes_a_snapshot() {
    let sources = SourceDatabase::default();
    let mut db = AnalysisDatabase::default();
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
    let mut db = AnalysisDatabase::default();
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
            .lookup("good")
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
    let mut db = AnalysisDatabase::default();
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
    let fresh = query(&mut AnalysisDatabase::default(), &sources);
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
    let mut db = AnalysisDatabase::default();
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
            .lookup("good")
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
            .lookup("old")
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
    let mut db = AnalysisDatabase::default();
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
    db.set_parse_limits(kagari_syntax::parser::ParseLimits {
        max_diagnostics: 0,
        ..Default::default()
    });
    let limited = db
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
    db.set_parse_limits(Default::default());
    let restored = db
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
