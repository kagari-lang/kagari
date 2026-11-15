use crate::analysis::*;
use crate::types::*;
use crate::{declarations::DeclarationId, hir::expr::ExprKind, tests::test_analysis};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::scalar::BuiltinType;

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn struct_update_facts_remap_rebase_and_invalidate_with_their_base_and_fields() {
    let text = "struct Item { val value: i32, val flag: bool } fn first() -> i32 { 1 } fn keep(base: Item, other: Item) -> Item { Item { value: 42, ..base } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("update-reuse.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let facts = authoring.records().facts();
    let DeclarationId::Definition(owner) = &facts
        .declarations
        .iter()
        .find(|declaration| declaration.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function declaration");
    };
    let owner = *owner;
    let body = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(body.diagnostics().is_empty());
    let old_init = body
        .lowered()
        .module
        .body
        .expressions()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::StructInit { .. }))
        .unwrap()
        .0;
    let old_target = body.type_table().struct_init(old_init).unwrap();
    assert_eq!(old_target.fields.len(), 1);
    assert_eq!(old_target.base_fields.len(), 1);
    let changed = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("update-reuse.kgr", changed.clone(), SourceLayer::Overlay)
        .unwrap();
    let reused = db
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().struct_init(old_init).is_none());
    let mut fresh_db = test_analysis();
    let declarations = fresh_db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let path = body.definitions().resolve(owner).unwrap().to_path();
    let fresh_owner = declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&path)
        .unwrap();
    let fresh = fresh_db
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        reused.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        reused.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    for text in [
        changed.replace("..base", "..other"),
        changed.replace("flag: bool", "flag: String"),
    ] {
        sources
            .set("update-reuse.kgr", text, SourceLayer::Overlay)
            .unwrap();
        let invalidated = db
            .body(sources.snapshot(), &owner, &Default::default())
            .unwrap()
            .unwrap();
        assert_eq!(invalidated.reused_bodies(), 0);
        assert!(
            invalidated.diagnostics().is_empty(),
            "{:?}",
            invalidated.diagnostics()
        );
    }
    let rebased = analyze(&mut db, &sources);
    let authoring = rebased
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let facts = authoring.records().facts();
    for (id, _) in facts.lowered.module.body.expressions() {
        if let Some(target) = facts.typed.type_table.struct_init(id) {
            let field = facts.aggregates.field(&target.base_fields[0]).unwrap();
            assert_eq!(field.name, "flag");
            assert_eq!(field.ty, TypeId::Builtin(BuiltinType::String));
        }
    }
    assert_eq!(
        body.type_table()
            .struct_init(old_init)
            .unwrap()
            .base_fields
            .len(),
        1
    );
}

#[test]
fn independent_body_queries_rebase_constructor_targets_and_invalidate_payload_changes() {
    let text =
        "enum Event { Data(i32) } fn first() -> i32 { 1 } fn keep() -> Event { Event::Data(7) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.records().facts();
    let DeclarationId::Definition(owner) = &old_facts
        .declarations
        .iter()
        .find(|d| d.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function")
    };
    let body = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(body.diagnostics().is_empty());
    assert_eq!(body.checked_bodies(), 1);
    let old_expr = body
        .lowered()
        .module
        .body
        .expressions()
        .find(|(id, e)| {
            matches!(e.kind, ExprKind::Call { .. })
                && body.type_table().enum_constructor(*id).is_some()
        })
        .unwrap()
        .0;
    let changed = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("cache.kgr", changed.clone(), SourceLayer::Overlay)
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().enum_constructor(old_expr).is_none());
    let mut fresh_db = test_analysis();
    let fresh_declarations = fresh_db
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let query = body.definitions().resolve(*owner).unwrap().to_path();
    let fresh_owner = fresh_declarations
        .file(file)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&query)
        .unwrap();
    let fresh = fresh_db
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        reused.definitions(),
        fresh.type_table(),
        fresh.definitions(),
        reused.lowered().module.body.arena(),
        fresh.lowered().module.body.arena(),
    );
    sources
        .set(
            "cache.kgr",
            changed.replace("Data(i32)", "Data(String)"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let invalidated = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(invalidated.reused_bodies(), 0);
    assert!(
        invalidated
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ArgumentTypeMismatch { .. }))
    );
    assert!(body.diagnostics().is_empty());
}
