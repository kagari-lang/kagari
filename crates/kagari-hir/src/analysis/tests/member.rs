use crate::analysis::*;
use crate::tests::test_analysis;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn checked_field_types_belong_to_their_own_lowering() {
    let text = "struct Point { var x: i32 } fn read(p: Point) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("field.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.records().facts();
    let old_field = old_facts.lowered.module.structs[0].fields[0].id;
    sources
        .set(
            "field.kgr",
            text.replace("{ p.x }", "{ p.x + 1 }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_file = new.file(file).unwrap();
    let authoring_new_facts = new_file.to_unverified(&Default::default()).unwrap();
    let new_facts = authoring_new_facts.records().facts();
    let new_field = new_facts.lowered.module.structs[0].fields[0].id;
    assert_ne!(old_field.arena(), new_field.arena());
    assert!(new_facts.typed.type_table.field_type(old_field).is_none());
    assert!(old_facts.typed.type_table.field_type(new_field).is_none());
    assert_eq!(
        new_facts.typed.type_table.field_type(new_field),
        old_facts.typed.type_table.field_type(old_field)
    );
    assert!(new_facts.typed.type_table.field_type(new_field).is_some());
    let fresh = analyze(&mut test_analysis(), &sources);
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.records().facts();
    new_facts.typed.type_table.assert_same_source_facts(
        authoring_new_facts.definitions(),
        &fresh.typed.type_table,
        authoring_fresh.definitions(),
        new_facts.lowered.module.nodes.arena(),
        fresh.lowered.module.nodes.arena(),
    );
}

#[test]
fn assignment_diagnostics_consume_existing_facts_without_rechecking_receivers() {
    for target in [
        "items[true].value",
        "items[true].absent",
        "items[true].nested[false].value",
    ] {
        let text = format!(
            "struct Item {{ val value: i32, val nested: [Item] }} fn bad(items: [Item]) {{ {target} = 1; }}"
        );
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("assignment-diagnostics.kgr", text, SourceLayer::Base)
            .unwrap();
        let snapshot = analyze(&mut test_analysis(), &sources);
        let file = snapshot.file(id).unwrap();
        let diagnostics = file.result().diagnostics();
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| matches!(d.kind, DiagnosticKind::InvalidIndexTarget { .. }))
                .count(),
            if target.contains("false") { 2 } else { 1 },
            "{target}: {diagnostics:?}"
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|d| matches!(d.kind, DiagnosticKind::InvalidAssignmentTarget { .. }))
                .count(),
            1
        );
        assert!(snapshot.check_program(id, &Default::default()).is_err());
    }
}
