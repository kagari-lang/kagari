use super::*;
use kagari_common::source_database::{SourceDatabase, SourceLayer};

fn analyze(db: &mut AnalysisDatabase, source: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(source.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn identical_local_slots_from_different_lowerings_cannot_resolve() {
    let mut sources = SourceDatabase::default();
    let text = "fn value(x: i32) -> i32 { var local = x; local += 1; match local { 2 => 3, other => other } }";
    let left_id = sources
        .set("left.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let right_id = sources
        .set("right.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let left = snapshot.file(left_id).unwrap().result().facts();
    let right = snapshot.file(right_id).unwrap().result().facts();
    assert_ne!(
        left.lowered.module.body.arena(),
        right.lowered.module.body.arena()
    );
    assert_eq!(
        left.lowered.module.body.arena(),
        left.lowered.source_map.arena()
    );
    let a = &left.lowered.module.functions[0];
    let b = &right.lowered.module.functions[0];
    assert_eq!(a.body.index(), b.body.index());
    assert_ne!(a.body, b.body);
    assert_eq!(a.params[0].id.index(), b.params[0].id.index());
    assert_ne!(a.params[0].id, b.params[0].id);
    assert_ne!(a.params[0].ty, b.params[0].ty);
    assert!(right.typed.type_table.type_ref(a.params[0].ty).is_none());
    assert!(
        right
            .declarations
            .target(crate::resolver::ResolvedName::Param(a.params[0].id))
            .is_none()
    );
    for (expr, _) in left.lowered.module.body.expressions() {
        assert!(right.typed.type_table.expr_type(expr).is_none());
        assert!(right.names.expr_resolution(expr).is_none());
    }
    let left_stmt = left.lowered.module.block(a.body).statements[0];
    let right_stmt = right.lowered.module.block(b.body).statements[0];
    assert_eq!(left_stmt.index(), right_stmt.index());
    assert_ne!(left_stmt, right_stmt);
    let crate::hir::StmtKind::Binding {
        local: left_local, ..
    } = left.lowered.module.stmt(left_stmt).kind
    else {
        panic!("binding");
    };
    let crate::hir::StmtKind::Binding {
        local: right_local, ..
    } = right.lowered.module.stmt(right_stmt).kind
    else {
        panic!("binding");
    };
    assert_eq!(left_local.index(), right_local.index());
    assert_ne!(left_local, right_local);
    assert!(right.typed.type_table.local_type(left_local).is_none());
    // Internal node/span access rejects foreign IDs before indexing by the slot.
    assert!(std::panic::catch_unwind(|| right.lowered.module.block(a.body)).is_err());
    assert!(
        std::panic::catch_unwind(|| right.lowered.source_map.param_span(a.params[0].id)).is_err()
    );
    assert!(std::panic::catch_unwind(|| right.lowered.module.stmt(left_stmt)).is_err());
}

#[test]
fn edits_retire_local_ids_while_unchanged_queries_share_the_lowering() {
    let mut sources = SourceDatabase::default();
    let text = "fn value(x: i32) -> i32 { x + 1 }";
    let id = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let again = analyze(&mut db, &sources);
    assert!(Arc::ptr_eq(old.file(id).unwrap(), again.file(id).unwrap()));
    let old_facts = old.file(id).unwrap().result().facts();
    let old_expr = old_facts
        .lowered
        .module
        .body
        .expressions()
        .find(|(expr, _)| old_facts.typed.type_table.expr_type(*expr).is_some())
        .unwrap()
        .0;
    sources
        .set("edit.kgr", text.replace("+ 1", "+ 2"), SourceLayer::Overlay)
        .unwrap();
    let edited = analyze(&mut db, &sources);
    let new = edited.file(id).unwrap().result().facts();
    assert_ne!(old_expr.arena(), new.lowered.module.body.arena());
    assert!(new.typed.type_table.expr_type(old_expr).is_none());
    assert!(old_facts.typed.type_table.expr_type(old_expr).is_some());
}

#[test]
fn interleaved_query_revisions_rebase_even_identical_source_revisions() {
    let mut sources = SourceDatabase::default();
    let text = "fn value(x: i32) -> i32 { val y: i32 = x; y }";
    let id = sources
        .set("stages.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let old_source = sources.snapshot();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_arena = old
        .file(id)
        .unwrap()
        .result()
        .facts()
        .lowered
        .module
        .body
        .arena();
    sources
        .set(
            "stages.kgr",
            text.replace("= x", "= x + 1"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let newest_declarations = db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    // Declaration cache now holds the edit, but signature/full caches hold the
    // original revision. Reconstructing that revision must not copy its old IDs.
    let reconstructed = db
        .snapshot(old_source, Default::default(), &Default::default())
        .unwrap();
    let file = reconstructed.file(id).unwrap();
    let arena = file.result().facts().lowered.module.body.arena();
    assert_ne!(old_arena, arena);
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let signatures = reconstructed.signature_snapshot().file(id).unwrap();
    assert!(Arc::ptr_eq(file.signatures(), signatures.signatures()));
    let signature = &file.signatures().facts().functions()[0];
    assert_eq!(signature.params[0].id.arena(), arena);
    let ty = file.result().facts().lowered.module.functions[0].params[0].ty;
    assert!(
        file.signatures()
            .facts()
            .type_table()
            .type_ref(ty)
            .is_some()
    );
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let latest = analyze(&mut db, &sources);
    latest
        .file(id)
        .unwrap()
        .signatures()
        .facts()
        .assert_same_source_facts(
            fresh.file(id).unwrap().signatures().facts(),
            latest
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .body
                .arena(),
            fresh
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .body
                .arena(),
        );
    assert!(Arc::ptr_eq(
        newest_declarations.file(id).unwrap(),
        latest.declaration_snapshot().file(id).unwrap()
    ));
}
