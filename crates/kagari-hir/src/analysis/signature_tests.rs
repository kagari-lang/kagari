use super::*;
use kagari_common::source_database::{SourceDatabase, SourceLayer};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

fn assert_fresh(file: &FileAnalysis, sources: &SourceDatabase) {
    let fresh = snapshot(&mut AnalysisDatabase::default(), sources);
    let fresh = fresh.file(file.source().id()).unwrap();
    assert_eq!(
        file.signatures().facts().functions(),
        fresh.signatures().facts().functions()
    );
    assert_eq!(
        file.signatures().facts().type_table(),
        fresh.signatures().facts().type_table()
    );
    assert_eq!(
        file.signatures().diagnostics(),
        fresh.signatures().diagnostics()
    );
    assert_eq!(file.result().diagnostics(), fresh.result().diagnostics());
    // Navigation must use this revision's declarations, including generic bounds.
    for (index, span) in file
        .result()
        .facts()
        .lowered
        .source_map
        .type_spans()
        .iter()
        .enumerate()
    {
        let id = crate::hir::TypeRefId::new(index);
        assert_eq!(
            file.result().facts().typed.type_table.type_ref(id),
            fresh.result().facts().typed.type_table.type_ref(id)
        );
        let location = |analysis: &FileAnalysis| {
            analysis
                .definition_at(span.start)
                .map(|d| (d.name.clone(), d.location))
        };
        assert_eq!(location(file), location(fresh));
    }
}

#[test]
fn body_edits_rebase_signature_types_and_preserve_old_queries() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() -> i32 { 1 }\r\nstruct Point { var x: i32 }\r\ntrait Show { fn show(self) -> i32; }\r\nimpl Show for Point { fn show(self) -> i32 { self.x } }\r\nfn keep<T: Show>(value: T) -> T { value }\r\nfn read(p: Point) -> i32 { p.x }";
    let id = sources
        .set("signatures.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = snapshot(&mut db, &sources);
    assert!(!first.file(id).unwrap().signatures_reused());
    assert!(
        first.file(id).unwrap().result().diagnostics().is_empty(),
        "{:?}",
        first.file(id).unwrap().result().diagnostics()
    );
    let old_target = first
        .file(id)
        .unwrap()
        .definition_at(text.rfind("Point").unwrap())
        .unwrap()
        .clone();
    let edit = text
        .replace(
            "{ 1 }",
            "{ val label: String = \"中文😀\"; val pair: (i32, bool) = (1, true); 2 }",
        )
        .replace("{ self.x }", "{ val added: [i32] = [2]; self.x }");
    sources
        .set("signatures.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    let file = second.file(id).unwrap();
    assert!(file.signatures_reused());
    assert_fresh(file, &sources);
    assert_eq!(first.declaration(&old_target.id), Some(&old_target));
    let new_target = second.declaration(&old_target.id).unwrap();
    assert_ne!(new_target.location, old_target.location);
    assert_eq!(new_target.location.revision, file.source().revision());
    // A real signature change invalidates reuse and subsequent bodies.
    sources
        .set(
            "signatures.kgr",
            edit.replace("var x: i32", "var x: bool"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = snapshot(&mut db, &sources);
    assert!(!third.file(id).unwrap().signatures_reused());
    assert_fresh(third.file(id).unwrap(), &sources);
}

#[test]
fn erroneous_signatures_reuse_with_rebased_diagnostics_and_fresh_body_errors() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() { missing; }\r\nfn bad(x: Absent) -> Absent { x }\r\npub fn generic<T>(x: T) -> T { x }";
    let id = sources
        .set("errors.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let first = snapshot(&mut db, &sources);
    assert!(
        !first
            .file(id)
            .unwrap()
            .signatures()
            .diagnostics()
            .is_empty()
    );
    let edit = text
        .replace("missing;", "val note: String = \"中文😀\";")
        .replace("-> T { x }", "-> T { unknown }");
    sources
        .set("errors.kgr", edit, SourceLayer::Overlay)
        .unwrap();
    let second = snapshot(&mut db, &sources);
    assert!(second.file(id).unwrap().signatures_reused());
    assert_fresh(second.file(id).unwrap(), &sources);
    assert_ne!(
        first.file(id).unwrap().signatures().diagnostics(),
        second.file(id).unwrap().signatures().diagnostics()
    );
}

#[test]
fn cancelled_or_older_queries_cannot_replace_signature_cache() {
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
    snapshot(&mut db, &sources);
    sources
        .set(
            "race.kgr",
            "fn value() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let latest = snapshot(&mut db, &sources);
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        db.snapshot(old_source.clone(), Default::default(), &cancel)
            .is_err()
    );
    let stale = db
        .snapshot(old_source, Default::default(), &Default::default())
        .unwrap();
    assert!(!stale.file(id).unwrap().signatures_reused());
    let again = snapshot(&mut db, &sources);
    assert!(Arc::ptr_eq(
        latest.file(id).unwrap(),
        again.file(id).unwrap()
    ));
    assert_fresh(again.file(id).unwrap(), &sources);
}
