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
fn signature_navigation_uses_checked_type_targets_before_body_analysis() {
    let text = "// 中文 😀\r\nstruct Box<T> { val data: T }\r\nfn bad(x: Box<Missing>) {}\r\nfn good<T>(value: Box<T>) -> Box<T> { missing }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("signature-targets.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let signatures = query(&mut db, &sources);
    let file = signatures.file(id).unwrap();
    assert_eq!(file.diagnostics().len(), 1);
    assert!(
        db.files.is_empty(),
        "signature query must not analyze bodies"
    );

    let bad_annotation = text.find("Box<Missing>").unwrap();
    let structure = file.definition_at(bad_annotation).unwrap();
    assert_eq!(structure.name, "Box");
    assert_eq!(
        &text[structure.location.range.start..structure.location.range.end],
        "Box"
    );
    assert!(file.definition_at(bad_annotation + "Box".len()).is_none());
    assert!(file.definition_at(text.find("Missing>").unwrap()).is_none());
    assert_eq!(
        file.definition_at(text.find("fn good").unwrap() + 3)
            .unwrap()
            .name,
        "good"
    );
    assert_eq!(
        file.definition_at(text.find("Box<T>)").unwrap() + "Box<".len())
            .unwrap()
            .location
            .range
            .start,
        text.find("good<T>").unwrap() + "good<".len()
    );
    assert!(
        file.definition_at(text.find("{ missing }").unwrap() + 2)
            .is_none()
    );

    let full = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(
        full.file(id).unwrap().definition_at(bad_annotation),
        Some(structure)
    );
}

#[test]
fn applied_bounds_are_signature_diagnostics_and_rebase_without_body_analysis() {
    let text = "fn before() {} struct Key<T: HashKey> { val value: T } struct Holder { val key: Key<f32> } enum Packet { Data(Key<f32>) } fn bad(x: Key<f32>) {} fn unresolved(x: Absent) {} fn good() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("applications.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let original = query(&mut db, &sources);
    let file = original.file(id).unwrap();
    assert_eq!(file.diagnostics().len(), 4, "{:?}", file.diagnostics());
    assert_eq!(file.signatures().diagnostics().len(), 4);
    assert_eq!(
        file.diagnostics()
            .iter()
            .filter(|d| d.kind.code() == "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED")
            .count(),
        3
    );
    assert!(db.files.is_empty());
    let same = query(&mut db, &sources);
    assert!(Arc::ptr_eq(file, same.file(id).unwrap()));
    let edit = text.replace("fn before() {}", "fn before() { val text = \"中文😀\"; }");
    sources
        .set("applications.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = query(&mut db, &sources);
    assert!(changed.file(id).unwrap().reused());
    let fresh = query(&mut AnalysisDatabase::default(), &sources);
    assert_eq!(
        changed.file(id).unwrap().diagnostics(),
        fresh.file(id).unwrap().diagnostics()
    );
    assert_eq!(changed.file(id).unwrap().diagnostics().len(), 4);
    assert_ne!(file.diagnostics(), changed.file(id).unwrap().diagnostics());
    let full = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert_eq!(full.file(id).unwrap().result().diagnostics().len(), 4);
    assert!(Arc::ptr_eq(
        changed.file(id).unwrap().signatures(),
        full.file(id).unwrap().signatures()
    ));
    sources
        .set(
            "applications.kgr",
            edit.replace("Key<f32>", "Key<i32>")
                .replace("Absent", "i32"),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(
        query(&mut db, &sources)
            .file(id)
            .unwrap()
            .diagnostics()
            .is_empty()
    );
    assert_eq!(file.diagnostics().len(), 4);
}

#[test]
fn imported_applied_bound_changes_invalidate_signature_diagnostics() {
    use kagari_common::identity::{ModuleIdentity, PackageId};
    let mut sources = SourceDatabase::default();
    for name in ["types", "facade", "user"] {
        sources
            .bind_module(
                &format!("mem://{name}"),
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
    }
    let types_id = sources
        .set(
            "mem://types",
            "pub struct Key<T> { val value: T }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    sources
        .set(
            "mem://facade",
            "pub use pkg::types::Key;".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let id = sources
        .set(
            "mem://user",
            "use pkg::facade::Key; fn accept(x: Key<f32>) {}".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let original = query(&mut db, &sources);
    assert!(original.file(id).unwrap().diagnostics().is_empty());
    let imported = original
        .file(id)
        .unwrap()
        .definition_at("use pkg::facade::Key; fn accept(x: ".len())
        .expect("imported signature target");
    assert_eq!(imported.name, "Key");
    assert_eq!(imported.location.file, types_id);
    sources
        .set(
            "mem://types",
            "pub struct Key<T: HashKey> { val value: T }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = query(&mut db, &sources);
    assert_eq!(changed.file(id).unwrap().diagnostics().len(), 1);
    assert_eq!(
        changed
            .file(id)
            .unwrap()
            .definition_at("use pkg::facade::Key; fn accept(x: ".len())
            .unwrap()
            .location
            .file,
        types_id
    );
    assert_eq!(
        changed.file(id).unwrap().diagnostics()[0].kind.code(),
        "KG_TYPE_STANDARD_CONSTRAINT_NOT_SATISFIED"
    );
    assert!(original.file(id).unwrap().diagnostics().is_empty());
    let fresh = query(&mut AnalysisDatabase::default(), &sources);
    assert_eq!(
        changed.file(id).unwrap().diagnostics(),
        fresh.file(id).unwrap().diagnostics()
    );
    sources
        .set(
            "mem://types",
            "pub struct Key<T> { val value: T }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    assert!(
        query(&mut db, &sources)
            .file(id)
            .unwrap()
            .diagnostics()
            .is_empty()
    );
    assert!(db.files.is_empty());
}

#[test]
fn signatures_own_constraints_for_shadowed_parameters_before_body_analysis() {
    use crate::typeck::ConstraintTarget;
    for header in ["impl<T: HashKey> Set<T>", "impl<T> Set<T> where T: HashKey"] {
        let text = format!(
            "trait Get {{ fn get(self) -> i32; }} {header} {{ fn size<T: Get>(self, value: T) -> i32 {{ value.get() }} }} fn bad() {{ missing }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources.set("bounds.kgr", text, SourceLayer::Base).unwrap();
        let mut db = AnalysisDatabase::default();
        let snapshot = query(&mut db, &sources);
        let file = snapshot.file(id).unwrap();
        assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
        let method = file
            .signatures()
            .facts()
            .functions()
            .iter()
            .find(|f| f.name == "size")
            .unwrap();
        let TypeId::Set(element) = &method.params[0].ty else {
            panic!("receiver");
        };
        let TypeId::Generic(outer) = element.as_ref() else {
            panic!("impl binder");
        };
        let TypeId::Generic(inner) = &method.params[1].ty else {
            panic!("method binder");
        };
        assert_ne!(outer, inner);
        assert!(matches!(
            method.bounds[outer].as_slice(),
            [ConstraintTarget::Standard(_)]
        ));
        assert!(matches!(
            method.bounds[inner].as_slice(),
            [ConstraintTarget::Trait(_)]
        ));
        assert_eq!(method.bounds.len(), 2);
        assert!(db.files.is_empty());
        let full = db
            .snapshot(sources.snapshot(), Default::default(), &Default::default())
            .unwrap();
        let analysis = full.file(id).unwrap();
        assert_eq!(analysis.result().diagnostics().len(), 1);
        assert_eq!(
            analysis.result().diagnostics()[0].kind.code(),
            "KG_RESOLVE_UNKNOWN_NAME"
        );
        let checked = analysis
            .result()
            .facts()
            .typed
            .functions
            .iter()
            .find(|f| f.name == "size")
            .unwrap();
        assert_eq!(method.bounds, checked.bounds);
    }
}

#[test]
fn cached_signature_bounds_survive_body_edits_and_bound_changes_invalidate_calls() {
    let text = "trait Get { fn get(self) -> i32; } fn pass<T: HashKey>(x: T) -> T { x } fn main() -> i32 { pass(7) }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("bound-edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = query(&mut db, &sources);
    let original = old
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    sources
        .set(
            "bound-edit.kgr",
            text.replace("{ x }", "{ val same = x; same }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let edited = query(&mut db, &sources);
    assert!(edited.file(id).unwrap().reused());
    let cached = edited
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(original.bounds, cached.bounds);
    let good = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    assert!(good.file(id).unwrap().result().diagnostics().is_empty());
    sources
        .set(
            "bound-edit.kgr",
            text.replace("T: HashKey", "T: Get"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = query(&mut db, &sources);
    assert!(!changed.file(id).unwrap().reused());
    let current = changed
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_ne!(original.bounds, current.bounds);
    let bad = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = bad.file(id).unwrap();
    assert_eq!(analysis.result().facts().typed.reused_bodies, 0);
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
    );
    let fresh = query(&mut AnalysisDatabase::default(), &sources);
    changed
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .assert_same_source_facts(
            fresh.file(id).unwrap().signatures().facts(),
            changed
                .file(id)
                .unwrap()
                .prepared
                .lowered
                .module
                .body
                .arena(),
            fresh.file(id).unwrap().prepared.lowered.module.body.arena(),
        );
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
