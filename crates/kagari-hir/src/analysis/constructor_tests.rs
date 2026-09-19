use super::*;
use crate::{declarations::DeclarationId, hir::ExprKind, types::BuiltinType};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn constructors_retain_nominal_targets_through_argument_errors() {
    let text = "enum Event { Empty, Data(i32, String) } fn good() -> Event { Event::Data(1, \"ok\") } fn empty() -> Event { Event::Empty } fn wrong() -> Event { Event::Data(true) } fn unknown() -> Event { Event::Absent(missing) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("constructors.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(file).unwrap();
    let facts = analysis.result().facts();
    let enumeration = facts.aggregates.enumerations().next().unwrap();
    let good = text.find("Event::Data").unwrap();
    let wrong = text.rfind("Event::Data").unwrap();
    assert_eq!(
        analysis.type_at(good),
        Some(TypeId::Enum(crate::types::NominalType {
            declaration: enumeration.id.clone(),
            arguments: Vec::new()
        }))
    );
    assert_eq!(analysis.type_at(wrong), analysis.type_at(good));
    assert_eq!(analysis.definition_at(good), analysis.definition_at(wrong));
    assert_eq!(
        analysis.definition_at(good),
        Some(&enumeration.variants[1].declaration)
    );
    assert_eq!(
        analysis.definition_at(text.find("Event::Empty").unwrap()),
        Some(&enumeration.variants[0].declaration)
    );
    assert!(
        analysis
            .definition_at(text.find("missing").unwrap())
            .is_none()
    );
    let codes = analysis
        .result()
        .diagnostics()
        .iter()
        .map(|d| d.kind.code())
        .collect::<Vec<_>>();
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "KG_TYPE_CALL_ARITY_MISMATCH")
            .count(),
        1
    );
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "KG_TYPE_ARGUMENT_TYPE_MISMATCH")
            .count(),
        1
    );
    let targets = facts
        .lowered
        .module
        .body
        .expressions()
        .filter_map(|(id, expr)| {
            matches!(expr.kind, ExprKind::Call { .. })
                .then(|| facts.typed.type_table.enum_constructor(id))
                .flatten()
        })
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 3);
    assert_eq!(
        targets
            .iter()
            .filter(|target| target.variant.is_none())
            .count(),
        1
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn source_facades_and_lexical_shadowing_do_not_confuse_constructor_owners() {
    let mut sources = SourceDatabase::default();
    let mut insert = |name: &str, text: &str| {
        let path = format!("memory://{name}");
        sources
            .bind_module(
                &path,
                ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec![name.into()],
                },
            )
            .unwrap();
        sources.set(&path, text.into(), SourceLayer::Base).unwrap()
    };
    let left = insert("left", "pub enum Event { Data(i32) }");
    let right = insert("right", "pub enum Event { Data(String) }");
    insert("facade", "pub use pkg::left::Event;");
    let text = "use pkg::facade; use pkg::right; use pkg::left::Event; fn a() -> Event { facade::Event::Data(1) } fn b() -> right::Event { right::Event::Data(\"ok\") } fn hidden(Event: i32) { Event::Data(1) }";
    let root = insert("root", text);
    let snapshot = analyze(&mut AnalysisDatabase::default(), &sources);
    let analysis = snapshot.file(root).unwrap();
    let a = analysis
        .definition_at(text.find("facade::Event::Data").unwrap())
        .unwrap();
    let b = analysis
        .definition_at(text.find("right::Event::Data").unwrap())
        .unwrap();
    assert_eq!(a.location.file, left);
    assert_eq!(b.location.file, right);
    assert_ne!(a.id, b.id);
    let hidden = text.rfind("Event::Data").unwrap();
    assert!(analysis.definition_at(hidden).is_none());
    assert_eq!(analysis.type_at(hidden), Some(TypeId::Error));
    assert!(
        analysis
            .result()
            .diagnostics()
            .iter()
            .all(|d| d.span.is_some_and(|span| span.start >= hidden))
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
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_facts = old.file(file).unwrap().result().facts();
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
    let fresh = AnalysisDatabase::default()
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    reused.type_table().assert_same_source_facts(
        fresh.type_table(),
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
    assert!(invalidated.diagnostics().iter().any(|d| matches!(
        d.kind,
        kagari_common::DiagnosticKind::ArgumentTypeMismatch { .. }
    )));
    assert!(body.diagnostics().is_empty());
    assert_eq!(
        body.type_at(text.find("(7)").unwrap() + 1),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}
