use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::span::Span;
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{declarations::DeclarationId, hir::ids::HirOwner};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn payload_errors_preserve_later_types_and_neighbor_queries() {
    let text = "struct Point { val x: i32 } enum Event { Empty, Data(Missing, (Missing, Point), i32), Hole(, Point) } fn bad() { unknown } fn good(p: Point) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("payload.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let signatures = db
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let signature = signatures.file(file).unwrap();
    assert!(
        signature
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_TYPE_UNKNOWN_ANNOTATION")
    );
    assert!(!signature.diagnostics().iter().any(|d| {
        d.span
            .is_some_and(|span| span.start == text.find("unknown").unwrap())
    }));
    let point_offset = text.find("Point)").unwrap();
    assert!(matches!(
        signature.queries().type_at(point_offset),
        Some(TypeId::Struct(_))
    ));
    assert_eq!(
        signature.queries().type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    assert_eq!(
        signature
            .queries()
            .type_at(text.find("i32), Hole").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let variants = &facts.lowered.module.enums[0].variants;
    assert_eq!(
        variants.iter().map(|v| v.payload.len()).collect::<Vec<_>>(),
        [0, 3, 2]
    );
    for ty in variants.iter().flat_map(|v| &v.payload) {
        assert_eq!(ty.owner(), HirOwner::Declaration);
        assert!(facts.typed.type_table().type_ref(*ty).is_some());
    }
    let missing = text.find(", Point) }").unwrap();
    assert_eq!(
        facts.lowered.source_map.type_span(variants[2].payload[0]),
        Span::new(missing, missing)
    );
    let DeclarationId::Definition(id) = &facts.declarations.variant(variants[2].id).unwrap().id
    else {
        panic!("variant identity")
    };
    let hole = facts.aggregates.variant(id).unwrap();
    assert_eq!(hole.payload[0], TypeId::Error);
    assert!(matches!(hole.payload[1], TypeId::Struct(_)));
    assert_eq!(
        analysis.queries().definition_at(point_offset).unwrap().name,
        "Point"
    );
    assert_eq!(
        analysis.queries().type_at(text.rfind("p.x").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}
