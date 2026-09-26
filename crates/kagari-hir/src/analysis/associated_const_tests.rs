use super::*;
use crate::{declarations::DeclarationId, typeck::ScalarValue, types::BuiltinType};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

#[test]
fn associated_constant_targets_survive_cached_body_rebasing_and_revision_changes() {
    let text = "trait Limit { const VALUE: i32 = 21; } struct N {} impl Limit for N {} fn edit() -> i32 { 1 } fn keep<T: Limit>(x: T) -> i32 { <T as Limit>::VALUE }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("cache.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let snapshot = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = snapshot.file(file).unwrap();
    assert!(analysis.result().diagnostics().is_empty());
    let declaration = analysis
        .definition_at(text.rfind("VALUE").unwrap())
        .unwrap();
    assert_eq!(
        declaration.location.range.start,
        text.find("VALUE").unwrap()
    );
    assert_eq!(
        analysis.type_at(text.rfind("VALUE").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let DeclarationId::Definition(owner) = &analysis
        .result()
        .facts()
        .declarations
        .iter()
        .find(|declaration| declaration.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function identity")
    };
    let old = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    let id = old
        .lowered()
        .module
        .body
        .expressions()
        .find(|(id, _)| old.type_table().associated_const(*id).is_some())
        .unwrap()
        .0;
    let edited = text.replace("{ 1 }", "{ val n = 2; n }");
    sources
        .set("cache.kgr", edited.clone(), SourceLayer::Overlay)
        .unwrap();
    let reused = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(reused.reused_bodies(), 1);
    assert!(reused.type_table().associated_const(id).is_none());
    assert!(
        reused
            .lowered()
            .module
            .body
            .expressions()
            .any(|(id, _)| reused.type_table().associated_const(id).is_some())
    );
    sources
        .set(
            "cache.kgr",
            edited.replace("= 21", "= 42"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let snapshot = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let facts = snapshot.file(file).unwrap().result().facts();
    assert!(
        facts
            .typed
            .const_values
            .values()
            .any(|value| value == &ScalarValue::I32(42))
    );
    assert!(
        analysis
            .result()
            .facts()
            .typed
            .const_values
            .values()
            .any(|value| value == &ScalarValue::I32(21))
    );
}
