use super::*;
use crate::{declarations::DeclarationId, tests::test_analysis};
use kagari_common::identity::{DefinitionPath, ModuleIdentity, PackageId};
use kagari_source::{
    diagnostic::DiagnosticKind,
    identity::FileId,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

fn owner(
    db: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    file: FileId,
    name: &str,
) -> DefinitionPath {
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let declaration = declarations
        .file(file)
        .unwrap()
        .declarations()
        .iter()
        .find(|d| d.name == name)
        .unwrap();
    let DeclarationId::Definition(id) = &declaration.id else {
        panic!("function definition");
    };
    declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .resolve(*id)
        .unwrap()
        .to_path()
}

fn query(
    db: &mut AnalysisDatabase,
    sources: &SourceDatabase,
    owner: &DefinitionPath,
) -> Arc<FunctionAnalysis> {
    db.declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let owner = db.definitions.snapshot().lookup(owner).unwrap();
    db.body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap()
}

#[test]
fn unrelated_applied_bound_errors_stay_in_the_signature_snapshot() {
    let text = "use std::hash::{Hash};\nstruct Key<T: Hash> { val value: T } fn bad(x: Key<f32>) {} fn good() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("application.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, file, "good");
    let result = query(&mut db, &sources, &good);
    assert_eq!(result.checked_bodies(), 1);
    assert_eq!(result.eligibility(), BodyEligibility::Unavailable);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert_eq!(
        result
            .signature_snapshot()
            .file(file)
            .unwrap()
            .diagnostics()
            .len(),
        1
    );
    assert_eq!(
        result.type_at(text.rfind('7').unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn selected_body_cache_refreshes_shared_signature_context() {
    let mut sources = SourceDatabase::default();
    for package in ["root", "other"] {
        sources
            .bind_module(
                &format!("mem://{package}"),
                ModuleIdentity {
                    package: PackageId(package.into()),
                    path: vec![],
                },
            )
            .unwrap();
    }
    let root_text = "fn good() -> i32 { 42 }";
    let valid_other = "fn helper() -> i32 { 2 }";
    let root = sources
        .set("mem://root", root_text.into(), SourceLayer::Base)
        .unwrap();
    let other = sources
        .set("mem://other", valid_other.into(), SourceLayer::Base)
        .unwrap();
    let original_source = sources.snapshot();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, root, "good");
    let original_query = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let original = query(&mut db, &sources, &good);
    let body_facts = Arc::downgrade(&original.facts);
    let root_lowering = Arc::downgrade(&original.file.prepared.lowered);
    let root_source = Arc::downgrade(&original.file.prepared.lowered.source);
    assert_eq!(original.eligibility(), BodyEligibility::Complete);
    assert!(Arc::ptr_eq(&original, &query(&mut db, &sources, &good)));

    let mut results = vec![original.clone()];
    for (text, eligibility) in [
        ("fn helper() -> Missing { 2 }", BodyEligibility::Unavailable),
        (valid_other, BodyEligibility::Complete),
    ] {
        sources
            .set("mem://other", text.into(), SourceLayer::Overlay)
            .unwrap();
        let updated = query(&mut db, &sources, &good);
        let fresh = query(&mut test_analysis(), &sources, &good);
        assert_eq!(updated.eligibility(), eligibility);
        assert_eq!(updated.eligibility(), fresh.eligibility());
        assert_eq!(updated.diagnostics(), fresh.diagnostics());
        assert_eq!(
            updated.type_at(root_text.find("42").unwrap()),
            fresh.type_at(root_text.find("42").unwrap())
        );
        assert_eq!(
            updated.signature_snapshot().revision(),
            sources.snapshot().revision()
        );
        let updated_other = updated.signature_snapshot().file(other).unwrap();
        assert_eq!(updated_other.source().text(), text);
        assert_eq!(
            updated_other.diagnostics(),
            fresh
                .signature_snapshot()
                .file(other)
                .unwrap()
                .diagnostics()
        );
        // Only the query context changed: retain the same immutable body facts.
        assert!(std::ptr::eq(original.type_table(), updated.type_table()));
        assert_eq!(updated.checked_bodies(), original.checked_bodies());
        assert_eq!(updated.reused_bodies(), original.reused_bodies());
        assert!(!Arc::ptr_eq(&original, &updated));
        assert!(Arc::ptr_eq(&updated, &query(&mut db, &sources, &good)));

        let old_request = db
            .body(
                original_source.clone(),
                original.owner(),
                &Default::default(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(old_request.eligibility(), BodyEligibility::Complete);
        assert_eq!(
            old_request.signature_snapshot().revision(),
            original_source.revision()
        );
        assert!(Arc::ptr_eq(&updated, &query(&mut db, &sources, &good)));
        results.push(updated);
    }

    assert_eq!(original.eligibility(), BodyEligibility::Complete);
    assert_eq!(
        original
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        valid_other
    );
    assert_eq!(results[1].eligibility(), BodyEligibility::Unavailable);
    assert_eq!(
        results[1]
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        "fn helper() -> Missing { 2 }"
    );
    assert_eq!(results[2].eligibility(), BodyEligibility::Complete);

    let old_query = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let old_other = Arc::downgrade(
        &results[2]
            .signature_snapshot()
            .file(other)
            .unwrap()
            .prepared
            .lowered,
    );
    let old_result = Arc::downgrade(&results[2]);
    sources
        .set(
            "mem://other",
            "fn helper() -> i32 { 3 }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let current_query = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let current_signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    assert_eq!(
        current_signatures.file(other).unwrap().source().text(),
        "fn helper() -> i32 { 3 }"
    );
    assert_eq!(old_query.file(other).unwrap().source().text(), valid_other);
    assert_eq!(
        results[2]
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        valid_other
    );
    assert!(old_other.upgrade().is_some());

    // Advance the full/signature caches without querying B: its body entry must
    // retain facts, but must not keep the previous whole-project query alive.
    let good_id = *original.owner();
    drop(original_query);
    drop(old_query);
    drop(original);
    drop(results);
    assert!(old_result.upgrade().is_none());
    assert!(old_other.upgrade().is_none());
    let cached = db.body_cache.get_id(good_id).unwrap();
    assert!(cached.result.upgrade().is_none());
    assert!(Arc::ptr_eq(&cached.facts, &body_facts.upgrade().unwrap()));
    assert!(Arc::ptr_eq(
        &root_lowering.upgrade().unwrap(),
        &current_query.file(root).unwrap().result().facts().lowered
    ));
    assert!(Arc::ptr_eq(
        &root_source.upgrade().unwrap(),
        &current_signatures
            .file(root)
            .unwrap()
            .prepared
            .lowered
            .source
    ));

    let rebuilt = query(&mut db, &sources, &good);
    assert!(Arc::ptr_eq(&rebuilt.facts, &body_facts.upgrade().unwrap()));
    assert_eq!(
        rebuilt.signature_snapshot().revision(),
        current_signatures.revision()
    );
    assert!(Arc::ptr_eq(
        rebuilt.signature_snapshot().file(other).unwrap(),
        current_signatures.file(other).unwrap()
    ));
    assert_eq!(
        rebuilt.type_at(root_text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(Arc::ptr_eq(&rebuilt, &query(&mut db, &sources, &good)));
    let returned = Arc::downgrade(&rebuilt);
    drop(rebuilt);
    assert!(returned.upgrade().is_none());
    let rebuilt = query(&mut db, &sources, &good);
    assert!(Arc::ptr_eq(&rebuilt.facts, &body_facts.upgrade().unwrap()));

    let old_expr = rebuilt
        .lowered()
        .module
        .body
        .expressions()
        .find(|(expr, _)| rebuilt.type_table().expr_type(*expr).is_some())
        .unwrap()
        .0;
    drop(current_query);
    drop(current_signatures);
    db.declaration_cache.take();
    db.signature_cache.take();
    db.files.clear();
    // A returned query alone still owns its exact graph and signature context.
    assert!(Arc::ptr_eq(
        &root_lowering.upgrade().unwrap(),
        &rebuilt.file.prepared.lowered
    ));
    assert_eq!(rebuilt.source().text(), root_text);
    assert_eq!(
        rebuilt
            .signature_snapshot()
            .file(other)
            .unwrap()
            .source()
            .text(),
        "fn helper() -> i32 { 3 }"
    );
    assert_eq!(
        rebuilt.type_at(root_text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    drop(rebuilt);
    let cached = db.body_cache.get_id(good_id).unwrap().clone();
    assert!(cached.result.upgrade().is_none());
    assert!(cached.file.upgrade().is_none());
    assert!(root_lowering.upgrade().is_none());
    assert!(Arc::ptr_eq(&cached.facts, &body_facts.upgrade().unwrap()));

    // The owned facts survive, but without their old graph they cannot supply IDs
    // to a new lowering. Rebuild from source rather than reviving that context.
    let rebuilt = query(&mut db, &sources, &good);
    assert!(!Arc::ptr_eq(&rebuilt.facts, &cached.facts));
    assert_ne!(rebuilt.lowered().module.body.arena(), old_expr.arena());
    assert!(rebuilt.type_table().expr_type(old_expr).is_none());
    assert!(root_lowering.upgrade().is_none());
    assert!(cached.file.upgrade().is_none());
    assert_eq!(rebuilt.checked_bodies(), 1);
    assert_eq!(rebuilt.reused_bodies(), 0);
    assert_eq!(rebuilt.eligibility(), BodyEligibility::Complete);
    assert!(rebuilt.diagnostics().is_empty());
    assert_eq!(
        rebuilt.type_at(root_text.find("42").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        rebuilt.signature_snapshot().revision(),
        sources.snapshot().revision()
    );
    assert!(Arc::ptr_eq(&rebuilt, &query(&mut db, &sources, &good)));
}

#[test]
fn single_function_query_does_not_check_or_bind_its_neighbors() {
    let mut sources = SourceDatabase::default();
    let text = "const N: i32 = 41; fn broken(x: i32) -> i32 { val wrong = missing; true } fn good(value: i32) -> i32 { val answer = value + N; answer }";
    let id = sources
        .set("single.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, id, "good");
    let result = query(&mut db, &sources, &good);
    assert_eq!(result.checked_bodies(), 1);
    assert_eq!(result.reused_bodies(), 0);
    assert_eq!(result.eligibility(), BodyEligibility::Complete);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert!(db.files.is_empty());
    assert_eq!(
        result.type_at(text.rfind("answer").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(result.type_at(text.find("missing").unwrap()).is_none());
    assert!(
        result
            .declarations()
            .iter()
            .all(|d| d.name != "wrong" && d.name != "x")
    );
    let local = result
        .declarations()
        .iter()
        .find(|d| d.name == "answer")
        .unwrap();
    assert!(matches!(local.id, DeclarationId::Binding(_)));
    let cached = query(&mut db, &sources, &good);
    assert!(Arc::ptr_eq(&result, &cached));
    let complete = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!complete.file(id).unwrap().result().diagnostics().is_empty());
    for (expr, _) in result.lowered().module.body.expressions() {
        if let Some(ty) = result.type_table().expr_type(expr) {
            assert_eq!(
                Some(ty),
                complete
                    .file(id)
                    .unwrap()
                    .result()
                    .facts()
                    .typed
                    .type_table
                    .expr_type(expr)
            );
        }
    }
}

#[test]
fn body_reuse_remaps_types_and_refreshes_local_identity_after_neighbor_edit() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() -> i32 { 1 } struct P { var n: i32 } fn good() -> i32 { val p: P = P { n: first() }; val P { n: answer } = p; answer }";
    let id = sources
        .set("reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let good = owner(&mut db, &sources, id, "good");
    let first = query(&mut db, &sources, &good);
    let old_local = first
        .declarations()
        .iter()
        .find(|d| d.name == "answer")
        .unwrap()
        .clone();
    let edit = text.replace(
        "{ 1 }",
        "{ val shifted: (i32, [i32]) = (10, [20]); missing }",
    );
    sources
        .set("reuse.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = query(&mut db, &sources, &good);
    assert_eq!(second.reused_bodies(), 1);
    assert_eq!(second.checked_bodies(), 0);
    assert!(second.diagnostics().is_empty());
    assert!(second.declarations().get(&old_local.id).is_none());
    assert_eq!(first.declarations().get(&old_local.id), Some(&old_local));
    let fresh = query(&mut test_analysis(), &sources, &good);
    second.type_table().assert_same_source_facts(
        second.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        second.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    assert_eq!(second.diagnostics(), fresh.diagnostics());
    sources
        .set(
            "reuse.kgr",
            edit.replace("var n: i32", "var n: bool"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let third = query(&mut db, &sources, &good);
    assert_eq!(third.checked_bodies(), 1);
    assert_eq!(third.reused_bodies(), 0);
    assert!(!third.diagnostics().is_empty());
    assert_eq!(third.eligibility(), BodyEligibility::Unavailable);
    assert!(db.files.is_empty());
}

#[test]
fn incomplete_member_query_keeps_receiver_and_constants_report_prerequisite_failures() {
    let mut sources = SourceDatabase::default();
    let text = "const BAD: i32 = 1 / 0; struct P { val n: i32 } fn member(p: P) { p. } fn unrelated() { missing; }";
    let id = sources
        .set("member.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let member = owner(&mut db, &sources, id, "member");
    let result = query(&mut db, &sources, &member);
    assert!(matches!(
        result.member_receiver_type(text.find("p. }").unwrap() + 2),
        Some(TypeId::Struct(_))
    ));
    assert_eq!(result.checked_bodies(), 1);
    assert!(!result.diagnostics().is_empty());
    assert!(
        result
            .diagnostics()
            .iter()
            .all(|d| !matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "missing"))
    );
}

#[test]
fn imported_signature_changes_invalidate_a_cached_function_body() {
    let mut sources = SourceDatabase::default();
    for name in ["dep", "root"] {
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
    let dependency = sources
        .set(
            "mem://dep",
            "pub fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let text = "use pkg::dep; fn value() -> i32 { dep::value() }";
    let root = sources
        .set("mem://root", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let dep_owner = owner(&mut db, &sources, dependency, "value");
    let root_owner = owner(&mut db, &sources, root, "value");
    assert_ne!(dep_owner, root_owner);
    let dep_body = query(&mut db, &sources, &dep_owner);
    let first = query(&mut db, &sources, &root_owner);
    assert!(first.diagnostics().is_empty());
    assert!(!Arc::ptr_eq(&dep_body, &first));
    sources
        .set(
            "mem://dep",
            "pub fn value() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let second = query(&mut db, &sources, &root_owner);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(second.checked_bodies(), 1);
    assert_eq!(second.reused_bodies(), 0);
    let call = text.find("dep::value()").unwrap();
    assert_eq!(first.type_at(call), Some(TypeId::Builtin(BuiltinType::I32)));
    assert_eq!(
        second.type_at(call),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    assert!(!second.diagnostics().is_empty());
    let fresh = query(&mut test_analysis(), &sources, &root_owner);
    second.type_table().assert_same_source_facts(
        second.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        second.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    assert_eq!(second.diagnostics(), fresh.diagnostics());
}

#[test]
fn same_named_impl_bodies_reuse_their_own_facts() {
    let mut sources = SourceDatabase::default();
    let text = "fn first() -> i32 { 1 } struct P { val n: i32 } struct Q { val n: bool } trait Show { fn show(self) -> i32; } impl Show for P { fn show(self) -> i32 { self.n } } impl Show for Q { fn show(self) -> i32 { if self.n { 7 } else { 8 } } }";
    let id = sources
        .set("methods.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file_id = id;
    let methods = declarations
        .file(id)
        .unwrap()
        .declarations()
        .iter()
        .filter_map(|d| {
            let DeclarationId::Definition(id) = &d.id else {
                return None;
            };
            let path = declarations
                .file(file_id)
                .unwrap()
                .declarations()
                .definitions()
                .resolve(*id)
                .unwrap()
                .to_path();
            (d.name == "show"
                && path
                    .path
                    .first()
                    .is_some_and(|p| p.kind == kagari_common::identity::DefinitionKind::Impl))
            .then_some(path)
        })
        .collect::<Vec<_>>();
    assert_eq!(methods.len(), 2);
    // Reuse/remapping needs caller-owned old graphs, not a strong cache reference.
    let mut previous = Vec::new();
    for method in &methods {
        let result = query(&mut db, &sources, method);
        assert_eq!(result.checked_bodies(), 1);
        assert!(
            result.diagnostics().is_empty(),
            "{:?}",
            result.diagnostics()
        );
        previous.push(result);
    }
    sources
        .set(
            "methods.kgr",
            text.replace("{ 1 }", "{ val shifted: [i32] = [1, 2]; shifted[0] }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    for (method, old) in methods.into_iter().zip(previous) {
        let result = query(&mut db, &sources, &method);
        assert_eq!(old.source().text(), text);
        assert_eq!(result.checked_bodies(), 0);
        assert_eq!(result.reused_bodies(), 1);
        let fresh = query(&mut test_analysis(), &sources, &method);
        result.type_table().assert_same_source_facts(
            result.definitions(),
            fresh.type_table(),
            fresh.definitions(),
            result.lowered().module.body.arena(),
            fresh.lowered().module.body.arena(),
        );
        assert_eq!(result.diagnostics(), fresh.diagnostics());
    }
}

#[test]
fn deleted_function_and_stale_queries_do_not_repopulate_latest_cache() {
    let mut sources = SourceDatabase::default();
    let id = sources
        .set(
            "delete.kgr",
            "fn value() -> i32 { 1 }".into(),
            SourceLayer::Base,
        )
        .unwrap();
    let mut db = test_analysis();
    let value = owner(&mut db, &sources, id, "value");
    let old_source = sources.snapshot();
    let old = query(&mut db, &sources, &value);
    let value = *old.owner();
    sources
        .set(
            "delete.kgr",
            "fn next() -> bool { true }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    db.declarations(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(!db.body_cache.get_id(value).is_some());
    assert!(
        db.body(sources.snapshot(), &value, &Default::default())
            .unwrap()
            .is_none()
    );
    assert!(!db.body_cache.get_id(value).is_some());
    let token = CancellationToken::default();
    token.cancel();
    assert!(db.body(old_source.clone(), &value, &token).is_err());
    let stale = db
        .body(old_source, &value, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(stale.source().revision(), old.source().revision());
    assert!(!db.body_cache.get_id(value).is_some());
    assert!(
        db.body(sources.snapshot(), &value, &Default::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn assignment_member_receivers_survive_errors_and_snapshot_revisions() {
    for target in ["p.inner.value", "p.inner.missing", "p.inner."] {
        let text = format!(
            "struct Inner {{ var value: i32 }} struct Outer {{ val inner: Inner }} fn edit(p: Outer) {{ {target} = 1; }} fn good() -> i32 {{ 42 }}"
        );
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("place-member.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let mut db = test_analysis();
        let edit = owner(&mut db, &sources, file, "edit");
        let original = query(&mut db, &sources, &edit);
        let cached = query(&mut db, &sources, &edit);
        assert!(Arc::ptr_eq(&original, &cached));
        let offset = text.find(target).unwrap() + "p.inner.".len();
        let receiver = original
            .member_receiver_type(offset)
            .expect("assignment receiver");
        assert!(
            matches!(&receiver, TypeId::Struct(ty) if original.definitions().resolve(ty.declaration).unwrap().last().unwrap().name == "Inner")
        );
        let valid = target == "p.inner.value";
        assert_eq!(original.diagnostics().is_empty(), valid);
        sources
            .set(
                "place-member.kgr",
                format!("// moved 😀\r\n{text}"),
                SourceLayer::Overlay,
            )
            .unwrap();
        let moved = query(&mut db, &sources, &edit);
        assert_eq!(moved.reused_bodies(), usize::from(valid));

        assert_eq!(
            moved.member_receiver_type(offset + "// moved 😀\r\n".len()),
            Some(receiver.clone())
        );
        assert_eq!(original.member_receiver_type(offset), Some(receiver));
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        assert_eq!(
            snapshot
                .file(file)
                .unwrap()
                .member_receiver_type(offset + "// moved 😀\r\n".len()),
            moved.member_receiver_type(offset + "// moved 😀\r\n".len())
        );
        assert_eq!(
            snapshot.check_program(file, &Default::default()).is_ok(),
            valid
        );
    }
}

#[test]
fn associated_definition_changes_invalidate_cached_body_and_keep_old_snapshot() {
    let text = "trait Read { type Item; fn read(self) -> Self::Item; } struct N {} impl Read for N { type Item = i32; fn read(self) -> Self::Item { 42 } } fn read<R: Read>(r: R) -> R::Item { r.read() } fn main() -> i32 { read(N {}) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("associated-edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let main = owner(&mut db, &sources, file, "main");
    let first = query(&mut db, &sources, &main);
    assert!(first.diagnostics().is_empty(), "{:?}", first.diagnostics());
    assert_eq!(
        first.type_at(text.rfind("read(N").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let edit = text
        .replace("type Item = i32", "type Item = bool")
        .replace("{ 42 }", "{ true }");
    sources
        .set("associated-edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let second = query(&mut db, &sources, &main);
    assert_eq!(second.checked_bodies(), 1);
    assert_eq!(second.reused_bodies(), 0);
    assert!(!second.diagnostics().is_empty());
    assert_eq!(
        second.type_at(edit.rfind("read(N").unwrap()),
        Some(TypeId::Builtin(BuiltinType::Bool))
    );
    assert!(first.diagnostics().is_empty());
    let fresh = query(&mut test_analysis(), &sources, &main);
    assert_eq!(second.diagnostics(), fresh.diagnostics());
}

#[test]
fn selected_local_function_reconstructs_static_ancestry_without_checking_parent() {
    let mut sources = SourceDatabase::default();
    let text = "fn outer(hidden: i32) { val broken = missing; fn inner() -> i32 { helper() } fn helper() -> i32 { 42 } }";
    let file = sources
        .set("local.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let inner = owner(&mut db, &sources, file, "inner");
    let result = query(&mut db, &sources, &inner);
    assert_eq!(result.checked_bodies(), 1);
    assert!(
        result.diagnostics().is_empty(),
        "{:?}",
        result.diagnostics()
    );
    assert_eq!(
        result.type_at(text.find("helper()").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let visible = result
        .names()
        .visible_bindings(text.find("helper()").unwrap());
    assert!(visible.iter().any(|binding| binding.name == "helper"));
    assert!(
        visible
            .iter()
            .all(|binding| binding.name != "hidden" && binding.name != "broken")
    );
    assert!(Arc::ptr_eq(&result, &query(&mut db, &sources, &inner)));
    let complete = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(
        complete
            .file(file)
            .unwrap()
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "missing"))
    );
}
