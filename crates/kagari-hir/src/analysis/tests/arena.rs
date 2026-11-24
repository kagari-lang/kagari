use crate::analysis::*;
use crate::{
    hir::{
        ids::{HirOwner, LocalId},
        stmt::StmtKind,
    },
    resolver::resolved::ResolvedName,
    tests::test_analysis,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn analyze(db: &mut AnalysisDatabase, source: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(source.snapshot(), &Default::default()).unwrap()
}

#[test]
fn identical_local_slots_from_different_lowerings_cannot_resolve() {
    let mut sources = SourceDatabase::default();
    let text = include_str!("../../../tests/fixtures/analysis/arena_bindings.kgr");
    let left_id = sources
        .set("left.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let right_id = sources
        .set("right.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
    let left = snapshot.file(left_id).unwrap().result().facts();
    let right = snapshot.file(right_id).unwrap().result().facts();
    assert_ne!(
        left.lowered.module.nodes.arena(),
        right.lowered.module.nodes.arena()
    );
    let a = &left.lowered.module.functions[0];
    let b = &right.lowered.module.functions[0];
    assert_eq!(
        a.implementation.script_body().unwrap().index(),
        b.implementation.script_body().unwrap().index()
    );
    assert_ne!(
        a.implementation.script_body().unwrap(),
        b.implementation.script_body().unwrap()
    );
    assert_eq!(a.params[0].id.index(), b.params[0].id.index());
    assert_ne!(a.params[0].id, b.params[0].id);
    assert_ne!(a.params[0].ty, b.params[0].ty);
    assert!(std::ptr::eq(
        left.lowered.module.parameter(a.params[0].id),
        &a.params[0]
    ));
    assert!(std::panic::catch_unwind(|| right.lowered.module.parameter(a.params[0].id)).is_err());
    assert!(right.typed.type_table.type_ref(a.params[0].ty).is_none());
    assert!(
        right
            .declarations
            .target(ResolvedName::Param(a.params[0].id))
            .is_none()
    );
    for (expr, _) in left.lowered.module.nodes.expressions() {
        assert!(right.typed.type_table.expr_type(expr).is_none());
        assert!(right.names.expr_resolution(expr).is_none());
    }
    let left_stmt = left
        .lowered
        .module
        .block(a.implementation.script_body().unwrap())
        .statements[0];
    let right_stmt = right
        .lowered
        .module
        .block(b.implementation.script_body().unwrap())
        .statements[0];
    assert_eq!(left_stmt.index(), right_stmt.index());
    assert_ne!(left_stmt, right_stmt);
    let StmtKind::Binding {
        pattern: left_pattern,
        ..
    } = left.lowered.module.stmt(left_stmt).kind
    else {
        panic!("binding");
    };
    let StmtKind::Binding {
        pattern: right_pattern,
        ..
    } = right.lowered.module.stmt(right_stmt).kind
    else {
        panic!("binding");
    };
    let left_local = left.lowered.module.nodes.pattern_bindings(left_pattern)[0].0;
    let right_local = right.lowered.module.nodes.pattern_bindings(right_pattern)[0].0;
    assert_eq!(left_pattern.index(), right_pattern.index());
    assert_ne!(left_pattern, right_pattern);
    assert_eq!(left_local.index(), right_local.index());
    assert_ne!(left_local, right_local);
    assert!(right.typed.type_table.local_type(left_local).is_none());
    let nodes = &left.lowered.module.nodes;
    let (id, name) = nodes.pattern_bindings(left_pattern)[0];
    assert_eq!(nodes.local(id).name, name);
    assert!(std::panic::catch_unwind(|| right.lowered.module.nodes.local(id)).is_err());
    let wrong_owner = LocalId::new(id.arena(), HirOwner::Declaration, id.index());
    assert!(std::panic::catch_unwind(|| nodes.local(wrong_owner)).is_err());
    // Internal node/span access rejects foreign IDs before indexing by the slot.
    assert!(
        std::panic::catch_unwind(|| right
            .lowered
            .module
            .block(a.implementation.script_body().unwrap()))
        .is_err()
    );
    assert!(
        std::panic::catch_unwind(|| right
            .lowered
            .module
            .parameter(a.params[0].id)
            .binding_span())
        .is_err()
    );
    assert!(std::panic::catch_unwind(|| right.lowered.module.stmt(left_stmt)).is_err());
    assert!(
        std::panic::catch_unwind(|| right.lowered.module.nodes.pattern_bindings(left_pattern))
            .is_err()
    );
}

#[test]
fn edited_lowerings_reject_old_local_ids_without_changing_retained_facts() {
    let mut sources = SourceDatabase::default();
    let text = "fn value(x: i32) -> i32 { x + 1 }";
    let id = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let old_facts = old.file(id).unwrap().result().facts();
    let old_expr = old_facts
        .lowered
        .module
        .nodes
        .expressions()
        .find(|(expr, _)| old_facts.typed.type_table.expr_type(*expr).is_some())
        .unwrap()
        .0;
    sources
        .set("edit.kgr", text.replace("+ 1", "+ 2"), SourceLayer::Overlay)
        .unwrap();
    let edited = analyze(&mut db, &sources);
    let new = edited.file(id).unwrap().result().facts();
    assert_ne!(old_expr.arena(), new.lowered.module.nodes.arena());
    assert!(new.typed.type_table.expr_type(old_expr).is_none());
    assert!(old_facts.typed.type_table.expr_type(old_expr).is_some());
    let old_parameter = old_facts.lowered.module.functions[0].params[0].id;
    let new_parameter = new.lowered.module.functions[0].params[0].id;
    assert_eq!((old_parameter.index(), new_parameter.index()), (0, 0));
    assert_eq!(old_facts.lowered.module.parameter(old_parameter).name, "x");
    assert_eq!(new.lowered.module.parameter(new_parameter).name, "x");
    assert!(std::panic::catch_unwind(|| new.lowered.module.parameter(old_parameter)).is_err());
    assert!(
        std::panic::catch_unwind(|| new.lowered.module.parameter(old_parameter).binding_span())
            .is_err()
    );
}

#[test]
fn interleaved_query_revisions_rebase_even_identical_source_revisions() {
    let mut sources = SourceDatabase::default();
    let text = "fn value(x: i32) -> i32 { val y: i32 = x; y }";
    let id = sources
        .set("stages.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let old_source = sources.snapshot();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let old_arena = old
        .file(id)
        .unwrap()
        .result()
        .facts()
        .lowered
        .module
        .nodes
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
    // The declaration cache now holds the edit. Reconstruct the old source
    // against its new lowering without using IDs from the retained result.
    let reconstructed = db.snapshot(old_source, &Default::default()).unwrap();
    let file = reconstructed.file(id).unwrap();
    let arena = file.result().facts().lowered.module.nodes.arena();
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
    let fresh = analyze(&mut test_analysis(), &sources);
    let latest = analyze(&mut db, &sources);
    latest
        .file(id)
        .unwrap()
        .signatures()
        .as_ref()
        .facts()
        .assert_same_source_facts(
            latest.file(id).unwrap().definitions(),
            fresh.file(id).unwrap().signatures().as_ref().facts(),
            fresh.file(id).unwrap().definitions(),
            latest
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .nodes
                .arena(),
            fresh
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .nodes
                .arena(),
        );
    assert!(Arc::ptr_eq(
        newest_declarations.file(id).unwrap(),
        latest.declaration_snapshot().file(id).unwrap()
    ));
}
