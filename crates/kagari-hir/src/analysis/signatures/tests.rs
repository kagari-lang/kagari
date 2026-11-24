use super::*;
use crate::{declarations::DeclarationId, tests::test_analysis, types::TypeId};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

use kagari_types::scalar::BuiltinType;

fn query(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> SignatureSnapshot {
    db.signatures(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn applied_bounds_are_signature_diagnostics_and_rebase_without_body_analysis() {
    let text = "use std::hash::{Hash};\nfn before() {} struct Key<T: Hash> { val value: T } struct Holder { val key: Key<f32> } enum Packet { Data(Key<f32>) } fn bad(x: Key<f32>) {} fn unresolved(x: Absent) {} fn good() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("applications.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let original = query(&mut db, &sources);
    let file = original.file(id).unwrap();
    assert_eq!(file.diagnostics().len(), 4, "{:?}", file.diagnostics());
    assert_eq!(file.signatures().diagnostics().len(), 4);
    assert_eq!(
        file.diagnostics()
            .iter()
            .filter(|d| d.kind.code() == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
            .count(),
        3
    );
    let edit = text.replace("fn before() {}", "fn before() { val text = \"中文😀\"; }");
    sources
        .set("applications.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = query(&mut db, &sources);
    let fresh = query(&mut test_analysis(), &sources);
    assert_eq!(
        changed.file(id).unwrap().diagnostics(),
        fresh.file(id).unwrap().diagnostics()
    );
    let changed_diagnostics = changed.file(id).unwrap().diagnostics();
    assert_eq!(changed_diagnostics.len(), 4);
    let shift = edit.len() - text.len();
    for (old, current) in file.diagnostics().iter().zip(changed_diagnostics) {
        assert_eq!(old.kind, current.kind);
        let old_span = old.span.unwrap();
        let current_span = current.span.unwrap();
        assert_eq!(
            (current_span.start, current_span.end),
            (old_span.start + shift, old_span.end + shift)
        );
        assert_eq!(
            &text[old_span.start..old_span.end],
            &edit[current_span.start..current_span.end]
        );
    }
    let full = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(full.file(id).unwrap().result().diagnostics().len(), 4);
    assert_eq!(
        full.signature_snapshot().file(id).unwrap().diagnostics(),
        changed_diagnostics
    );
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
    let _types_id = sources
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
    let mut db = test_analysis();
    let original = query(&mut db, &sources);
    assert!(original.file(id).unwrap().diagnostics().is_empty());

    sources
        .set(
            "mem://types",
            "use std::hash::Hash; pub struct Key<T: Hash> { val value: T }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = query(&mut db, &sources);
    assert_eq!(changed.file(id).unwrap().diagnostics().len(), 1);

    assert_eq!(
        changed.file(id).unwrap().diagnostics()[0].kind.code(),
        "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED"
    );
    assert!(original.file(id).unwrap().diagnostics().is_empty());
    let fresh = query(&mut test_analysis(), &sources);
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
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn signatures_own_constraints_for_shadowed_parameters_before_body_analysis() {
    for header in [
        "impl<T: Eq + Hash> HashSet<T>",
        "impl<T> HashSet<T> where T: Eq + Hash",
    ] {
        let text = format!(
            "use std::collections::HashSet; use std::hash::Hash; trait Get {{ fn get(self) -> i32; }} {header} {{ fn size<T: Get>(self, value: T) -> i32 {{ value.get() }} }} fn bad() {{ missing }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources.set("bounds.kgr", text, SourceLayer::Base).unwrap();
        let mut db = test_analysis();
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
        let TypeId::NativeObject(receiver) = &method.params[0].ty else {
            panic!("receiver");
        };
        let TypeId::Generic(outer) = &receiver.arguments[0] else {
            panic!("impl binder");
        };
        let TypeId::Generic(inner) = &method.params[1].ty else {
            panic!("method binder");
        };
        assert_ne!(outer, inner);
        assert!(matches!(
            method.bounds[&crate::types::TypeId::Generic(outer.clone())].as_slice(),
            [_, _]
        ));
        assert!(matches!(
            method.bounds[&crate::types::TypeId::Generic(inner.clone())].as_slice(),
            [_]
        ));
        assert_eq!(method.bounds.len(), 2);
        let full = db
            .snapshot(sources.snapshot(), &Default::default())
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
            .signatures
            .functions()
            .iter()
            .find(|f| f.name == "size")
            .unwrap();
        assert_eq!(method.bounds, checked.bounds);
    }
}
*/

#[test]
fn signature_bounds_stay_stable_across_body_edits_and_govern_calls() {
    let text = "use std::hash::{Hash};\ntrait Get { fn get(self) -> i32; } fn pass<T: Eq + Hash>(x: T) -> T { x } fn main() -> i32 { pass(7) }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("bound-edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
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
    let unchanged = edited
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    assert_eq!(original.bounds, unchanged.bounds);
    let good = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(good.file(id).unwrap().result().diagnostics().is_empty());
    sources
        .set(
            "bound-edit.kgr",
            text.replace("T: Eq + Hash", "T: Get"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let changed = query(&mut db, &sources);
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
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let analysis = bad.file(id).unwrap();
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED")
    );
    let fresh = query(&mut test_analysis(), &sources);
    changed
        .file(id)
        .unwrap()
        .signatures()
        .as_ref()
        .facts()
        .assert_same_source_facts(
            changed.file(id).unwrap().definitions(),
            fresh.file(id).unwrap().signatures().as_ref().facts(),
            fresh.file(id).unwrap().definitions(),
            changed
                .file(id)
                .unwrap()
                .prepared
                .lowered
                .module
                .nodes
                .arena(),
            fresh
                .file(id)
                .unwrap()
                .prepared
                .lowered
                .module
                .nodes
                .arena(),
        );
}

#[test]
fn independent_signature_query_preserves_errors_without_body_analysis() {
    let mut sources = SourceDatabase::default();
    let text = "fn bad(x: Absent) -> Absent { missing } const BAD: i32 = 1 / 0; fn good(x: i32) -> i32 { val local = x; local }";
    let id = sources
        .set("signatures.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let signatures = query(&mut db, &sources);
    let file = signatures.file(id).unwrap();
    let declarations = signatures.declaration_snapshot().file(id).unwrap();
    assert!(std::ptr::eq(file.lowered(), declarations.lowered()));
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

    let full = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        file.diagnostics(),
        full.signature_snapshot().file(id).unwrap().diagnostics()
    );
    assert!(Arc::ptr_eq(
        full.signature_snapshot().file(id).unwrap().signatures(),
        full.file(id).unwrap().signatures()
    ));
    assert!(std::ptr::eq(
        full.declaration_snapshot().file(id).unwrap().lowered(),
        full.file(id).unwrap().result().facts().lowered.as_ref(),
    ));
    let diagnostics = full.file(id).unwrap().result().diagnostics();
    assert!(
        diagnostics
            .iter()
            .any(|d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "missing"))
    );
    assert!(diagnostics.iter().any(|d| matches!(&d.kind, DiagnosticKind::InvalidConstInitializer { const_name, .. } if const_name == "BAD")));
}

#[test]
fn signatures_add_imported_type_facts_without_changing_declarations() {
    let mut sources = SourceDatabase::default();
    let mut ids = Vec::new();
    let user = include_str!("../../../tests/fixtures/analysis/records/user.kgr");
    for (name, text) in [
        (
            "types",
            include_str!("../../../tests/fixtures/analysis/records/types.kgr"),
        ),
        ("user", user),
    ] {
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
        ids.push(sources.set(&path, text.into(), SourceLayer::Base).unwrap());
    }
    let [provider, user]: [FileId; 2] = ids.try_into().unwrap();
    let mut db = AnalysisDatabase::default();
    let checked = query(&mut db, &sources);
    let declarations = checked.declaration_snapshot();
    let headers = declarations.file(user).unwrap();
    let file = checked.file(user).unwrap();
    assert!(file.diagnostics().is_empty(), "{:?}", file.diagnostics());
    assert_ne!(
        file.declarations().imported_types(),
        headers.declarations().imported_types()
    );
    let data = declarations
        .file(provider)
        .unwrap()
        .declarations()
        .iter()
        .find(|declaration| declaration.name == "Data")
        .unwrap();
    let pass = file
        .signatures()
        .facts()
        .functions()
        .iter()
        .find(|f| f.name == "pass")
        .unwrap();
    let TypeId::Struct(nominal) = &pass.return_type else {
        panic!("expected the imported Data type");
    };
    assert_eq!(data.id, DeclarationId::Definition(nominal.declaration));
    assert_eq!(pass.params[0].ty, pass.return_type);
}

#[test]
fn older_or_cancelled_signature_requests_preserve_each_revisions_facts() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "race.kgr",
            "fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let old_source = sources.snapshot();
    let mut db = test_analysis();
    let original = query(&mut db, &sources);
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
    assert!(matches!(
        db.signatures(old_source.clone(), &token),
        Err(AnalysisError::Cancelled)
    ));
    let stale = db.signatures(old_source, &Default::default()).unwrap();
    let again = query(&mut db, &sources);
    assert_eq!(stale.revision(), original.revision());
    assert_eq!(again.revision(), latest.revision());
    for (snapshot, expected) in [
        (&original, BuiltinType::I32),
        (&stale, BuiltinType::I32),
        (&latest, BuiltinType::Bool),
        (&again, BuiltinType::Bool),
    ] {
        let signature = &snapshot.file(id).unwrap().signatures().facts().functions()[0];
        assert_eq!(signature.return_type, TypeId::Builtin(expected));
    }
}
