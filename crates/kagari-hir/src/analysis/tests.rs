use super::*;
use kagari_contract::scalar::BuiltinType;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

#[test]
fn body_edits_reuse_other_bodies_but_signatures_invalidate_them() {
    let mut sources = SourceDatabase::default();
    let file = sources
        .set(
            "body.kgr",
            "fn a() -> i32 { 1 } fn b() -> i32 { a() }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let token = CancellationToken::default();
    let first = db.snapshot(sources.snapshot(), &token).unwrap();
    assert_eq!(
        first
            .file(file)
            .unwrap()
            .result()
            .facts()
            .typed
            .checked_bodies,
        2
    );
    sources
        .set(
            "body.kgr",
            "fn a() -> i32 { 100 + 2 } fn b() -> i32 { a() }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = db.snapshot(sources.snapshot(), &token).unwrap();
    let result = second.file(file).unwrap().result();
    assert_eq!(result.facts().typed.reused_bodies, 1);
    assert_eq!(result.facts().typed.checked_bodies, 1);
    assert!(result.clone().into_codegen().is_ok());
    sources
        .set(
            "body.kgr",
            "fn a() -> bool { true } fn b() -> i32 { a() }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = db.snapshot(sources.snapshot(), &token).unwrap();
    let result = third.file(file).unwrap().result();
    assert_eq!(result.facts().typed.reused_bodies, 0);
    assert!(!result.diagnostics().is_empty());
}

#[test]
fn broken_body_preserves_neighbor_and_member_receiver() {
    let mut sources = SourceDatabase::default();
    let text = "struct P { var n: i32 } fn bad() { val p = P { n: 1 }; p. } fn good() -> i32 { val answer = 42; answer }";
    let file = sources
        .set("a.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = AnalysisDatabase::default()
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let facts = snapshot.file(file).unwrap();
    assert!(!facts.result().diagnostics().is_empty());
    assert_eq!(
        facts.member_receiver_type(text.find("p. }").unwrap() + 2),
        Some(TypeId::Struct(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: facts
                .definitions()
                .resolve(
                    *facts
                        .result()
                        .facts()
                        .declarations
                        .definition(ResolvedName::Struct(StructId::new(0)))
                        .unwrap()
                )
                .unwrap()
                .to_path(),
            arguments: Vec::new(),
        }))
    );
    let offset = text.rfind("answer").unwrap();
    assert_eq!(
        facts.type_at(offset),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        facts
            .visible_bindings(offset)
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        vec!["answer"]
    );
    assert!(facts.result().clone().into_codegen().is_err());
}

#[test]
fn snapshots_reuse_unchanged_files_and_cancellation_does_not_publish() {
    let mut sources = SourceDatabase::default();
    let a = sources
        .set("a.kgr", "fn a() -> i32 { 1 }".into(), SourceLayer::Base)
        .unwrap();
    let b = sources
        .set("b.kgr", "fn b() -> i32 { 2 }".into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = db
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    sources
        .set("b.kgr", "fn b() -> i32 { 3 }".into(), SourceLayer::Overlay)
        .unwrap();
    let second = db
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    assert!(Arc::ptr_eq(first.file(a).unwrap(), second.file(a).unwrap()));
    assert!(!Arc::ptr_eq(
        first.file(b).unwrap(),
        second.file(b).unwrap()
    ));
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(db.snapshot(sources.snapshot(), &cancel).is_err());
    assert!(first.file(b).unwrap().source().text().contains("{ 2 }"));
}
