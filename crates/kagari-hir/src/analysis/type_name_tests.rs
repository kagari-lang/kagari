use super::*;
use crate::{declarations::DeclarationId, resolver::NameResolution, types::BuiltinType};
use kagari_common::source_database::{SourceDatabase, SourceLayer};

#[test]
fn duplicate_declarations_have_no_winner_in_any_semantic_consumer() {
    let declarations = [
        "struct Clash {}",
        "enum Clash { Ready }",
        "trait Clash {}",
        "fn Clash() -> i32 { 1 }",
        "const Clash: i32 = 2;",
        "mod Clash;",
    ];
    for first in declarations {
        for second in declarations {
            let text = format!(
                "{first} {second} struct Valid {{}} fn bad(x: Clash) -> Clash {{ Clash::Ready }} fn make() {{ val x = Clash {{}}; }} fn call() {{ Clash(); }} fn read() {{ Clash; }} fn bound<T>(x: T) -> T where T: Clash {{ x }} impl Clash for Valid {{}} fn good(x: i32) -> i32 {{ x }}"
            );
            let mut sources = SourceDatabase::default();
            let file = sources
                .set("collision.kgr", text.clone(), SourceLayer::Base)
                .unwrap();
            let mut db = AnalysisDatabase::default();
            let headers = db
                .declarations(sources.snapshot(), &Default::default())
                .unwrap();
            let header = headers.file(file).unwrap();
            assert_eq!(
                header.names().items.lookup("Clash"),
                Some(NameResolution::Ambiguous)
            );
            let duplicates = header
                .diagnostics()
                .iter()
                .filter(|d| d.kind.code() == "KG_RESOLVE_DUPLICATE_DECLARATION")
                .collect::<Vec<_>>();
            assert_eq!(duplicates.len(), 1, "{text}");
            assert_eq!(duplicates[0].span.unwrap().start, first.len() + 1);
            let retained = header
                .declarations()
                .iter()
                .filter(|d| d.name == "Clash")
                .collect::<Vec<_>>();
            assert_eq!(retained.len(), 2);
            assert_ne!(retained[0].id, retained[1].id);

            let snapshot = db
                .snapshot(sources.snapshot(), Default::default(), &Default::default())
                .unwrap();
            let analysis = snapshot.file(file).unwrap();
            let annotation = text.find("x: Clash").unwrap() + 3;
            assert_eq!(analysis.type_at(annotation), Some(TypeId::Error));
            assert!(analysis.definition_at(annotation).is_none());
            assert!(
                analysis
                    .definition_at(text.find("{ Clash();").unwrap() + 2)
                    .is_none()
            );
            assert!(
                analysis
                    .definition_at(text.find("{ Clash;").unwrap() + 2)
                    .is_none()
            );
            assert!(
                analysis
                    .definition_at(text.find("Clash::Ready").unwrap())
                    .is_none()
            );
            assert!(
                analysis
                    .definition_at(text.find("T: Clash").unwrap() + 3)
                    .is_none()
            );
            let diagnostics = analysis.result().diagnostics();
            let facts = analysis.result().facts();
            let valid = facts
                .aggregates
                .structures()
                .find(|item| item.declaration.name == "Valid")
                .unwrap();
            for item in &facts.lowered.module.traits {
                assert!(
                    !facts.typed.type_table.implements(
                        &crate::types::NominalType {
                            declaration: facts
                                .declarations
                                .definition(crate::resolver::ResolvedName::Trait(item.id))
                                .unwrap()
                                .clone(),
                            arguments: Vec::new(),
                        },
                        &TypeId::Struct(crate::types::NominalType {
                            declaration: valid.id.clone(),
                            arguments: Vec::new()
                        })
                    )
                );
            }
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.kind.code() == "KG_TYPE_INVALID_STRUCT_INITIALIZER"),
                "{diagnostics:?}"
            );
            assert!(
                diagnostics
                    .iter()
                    .any(|d| d.kind.code() == "KG_TYPE_UNKNOWN_TRAIT"),
                "{diagnostics:?}"
            );
            let good_use = text.rfind(" x }").unwrap() + 1;
            assert_eq!(
                analysis.type_at(good_use),
                Some(TypeId::Builtin(BuiltinType::I32))
            );
            assert!(analysis.definition_at(good_use).is_some());
            assert!(analysis.result().clone().into_codegen().is_err());
        }
    }
}

#[test]
fn introducing_and_removing_a_type_collision_invalidates_cached_targets() {
    let text = "enum Event { Ready } fn keep() -> Event { Event::Ready }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = db
        .snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap();
    let analysis = old.file(file).unwrap();
    let DeclarationId::Definition(owner) = &analysis
        .result()
        .facts()
        .declarations
        .iter()
        .find(|d| d.name == "keep")
        .unwrap()
        .id
    else {
        panic!("function definition");
    };
    let original = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(original.diagnostics().is_empty());
    sources
        .set(
            "edit.kgr",
            format!("{text} struct Event {{}}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let broken = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(broken.reused_bodies(), 0);
    assert_eq!(
        broken.type_at(text.find("-> Event").unwrap() + 3),
        Some(TypeId::Error)
    );
    assert!(
        db.declarations(sources.snapshot(), &Default::default())
            .unwrap()
            .file(file)
            .unwrap()
            .diagnostics()
            .iter()
            .any(|d| d.kind.code() == "KG_RESOLVE_DUPLICATE_DECLARATION")
    );
    sources
        .set("edit.kgr", text.into(), SourceLayer::Overlay)
        .unwrap();
    let repaired = db
        .body(sources.snapshot(), owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(repaired.reused_bodies(), 0);
    assert!(
        repaired.diagnostics().is_empty(),
        "{:?}",
        repaired.diagnostics()
    );
    assert_eq!(
        repaired.type_at(text.find("Event::Ready").unwrap()),
        original.type_at(text.find("Event::Ready").unwrap())
    );
    assert!(
        analysis
            .definition_at(text.find("Event::Ready").unwrap() + "Event::".len())
            .is_some()
    );
    assert!(analysis.result().diagnostics().is_empty());
}
