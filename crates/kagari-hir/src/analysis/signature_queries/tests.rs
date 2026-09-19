use super::*;
use crate::{
    declarations::DeclarationId,
    types::{BuiltinType, TypeId},
};
use kagari_common::{
    DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

fn query(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> SignatureSnapshot {
    db.signatures(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn independent_signature_query_preserves_errors_without_body_analysis() {
    let mut sources = SourceDatabase::default();
    let text = "fn bad(x: Absent) -> Absent { missing } const BAD: i32 = 1 / 0; fn good(x: i32) -> i32 { val local = x; local }";
    let id = sources
        .set("signatures.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let signatures = query(&mut db, &sources);
    let file = signatures.file(id).unwrap();
    assert!(!file.reused());
    assert!(Arc::ptr_eq(
        declarations.file(id).unwrap(),
        signatures.declaration_snapshot().file(id).unwrap()
    ));
    assert!(
        file.declarations()
            .iter()
            .all(|d| !matches!(d.id, DeclarationId::Binding(_)))
    );
    assert_eq!(file.diagnostics().len(), 2);
    assert!(
        file.diagnostics()
            .iter()
            .all(|d| matches!(d.kind, DiagnosticKind::UnknownType { .. }))
    );
    let good = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "good")
        .unwrap();
    assert_eq!(good.return_type, TypeId::Builtin(BuiltinType::I32));
    assert!(db.files.is_empty());
    let full = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        file,
        full.signature_snapshot().file(id).unwrap()
    ));
    assert!(Arc::ptr_eq(
        file.signatures(),
        full.file(id).unwrap().signatures()
    ));
    assert!(Arc::ptr_eq(
        &file.prepared.lowered,
        &full.file(id).unwrap().result().facts().lowered
    ));
    assert!(Arc::ptr_eq(
        &file.prepared.lowered,
        &declarations.file(id).unwrap().declared.lowered
    ));
    assert!(full.file(id).unwrap().result().diagnostics().len() > file.diagnostics().len());
    let again = query(&mut db, &sources);
    assert!(Arc::ptr_eq(file, again.file(id).unwrap()));
}

#[test]
fn signature_cache_reuses_and_rebases_without_any_complete_analysis() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() { missing; }\r\nfn good(x: i32) -> i32 { x }\r\nfn bad(x: Absent) -> Absent { x }";
    let id = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = query(&mut db, &sources);
    let edit = text.replace("missing;", "val note: String = \"中文😀\";");
    sources
        .set("edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = query(&mut db, &sources);
    assert!(second.file(id).unwrap().reused());
    let fresh = query(&mut AnalysisDatabase::default(), &sources);
    second
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .assert_same_source_facts(
            fresh.file(id).unwrap().signatures().facts(),
            second
                .file(id)
                .unwrap()
                .prepared
                .lowered
                .module
                .body
                .arena(),
            fresh.file(id).unwrap().prepared.lowered.module.body.arena(),
        );
    assert_eq!(
        second.file(id).unwrap().diagnostics(),
        fresh.file(id).unwrap().diagnostics()
    );
    assert_ne!(
        first.file(id).unwrap().diagnostics(),
        second.file(id).unwrap().diagnostics()
    );
    sources
        .set(
            "edit.kgr",
            edit.replace("-> i32", "-> bool"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = query(&mut db, &sources);
    assert!(!third.file(id).unwrap().reused());
    let good = third
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "good")
        .unwrap();
    assert_eq!(good.return_type, TypeId::Builtin(BuiltinType::Bool));
    assert!(db.files.is_empty());
}

#[test]
fn older_or_cancelled_signature_queries_do_not_replace_newer_caches() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "race.kgr",
            "fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let old_source = sources.snapshot();
    let mut db = AnalysisDatabase::default();
    query(&mut db, &sources);
    sources
        .set(
            "race.kgr",
            "fn value() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let latest = query(&mut db, &sources);
    let token = CancellationToken::default();
    token.cancel();
    assert!(db.signatures(old_source.clone(), &token).is_err());
    let stale = db.signatures(old_source, &Default::default()).unwrap();
    assert!(stale.revision() < latest.revision());
    let again = query(&mut db, &sources);
    assert!(Arc::ptr_eq(
        latest.file(id).unwrap(),
        again.file(id).unwrap()
    ));
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(Arc::ptr_eq(
        latest.declaration_snapshot().file(id).unwrap(),
        declarations.file(id).unwrap()
    ));
    assert!(db.files.is_empty());
}
