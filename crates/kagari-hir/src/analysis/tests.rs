use super::*;
use crate::tests::test_analysis;
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
    let mut db = test_analysis();
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
fn snapshots_reuse_unchanged_files_and_cancellation_does_not_publish() {
    let mut sources = SourceDatabase::default();
    let a = sources
        .set("a.kgr", "fn a() -> i32 { 1 }".into(), SourceLayer::Base)
        .unwrap();
    let b = sources
        .set("b.kgr", "fn b() -> i32 { 2 }".into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
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

mod arena;
mod associated_const;
mod callable;
mod completion;
mod constructor;
mod gat;
mod generic_type;
mod identity;
mod member;
mod namespace;
mod owner;
mod payload;
mod prelude;
mod scoped_identity;
mod signature;
mod standard_query;
mod statics;
mod trait_catalog;
mod trait_identity;
mod trait_reference;
mod type_application;
mod type_name;
