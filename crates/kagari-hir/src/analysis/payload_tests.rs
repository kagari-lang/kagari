use super::*;
use crate::{declarations::DeclarationId, hir::HirOwner, types::BuiltinType};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    source_database::{SourceDatabase, SourceLayer},
};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), Default::default(), &Default::default())
        .unwrap()
}

#[test]
fn payload_errors_preserve_later_types_and_neighbor_queries() {
    let text = "struct Point { val x: i32 } enum Event { Empty, Data(Missing, (Missing, Point), i32), Hole(, Point) } fn bad() { unknown } fn good(p: Point) -> i32 { p.x }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("payload.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
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
        signature.type_at(point_offset),
        Some(TypeId::Struct(_))
    ));
    assert_eq!(
        signature.type_at(text.find("Missing").unwrap()),
        Some(TypeId::Error)
    );
    assert_eq!(
        signature.type_at(text.find("i32), Hole").unwrap()),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    let snapshot = analyze(&mut db, &sources);
    let analysis = snapshot.file(file).unwrap();
    let facts = analysis.result().facts();
    let variants = &facts.lowered.module.enums[0].variants;
    assert_eq!(
        variants.iter().map(|v| v.payload.len()).collect::<Vec<_>>(),
        [0, 3, 2]
    );
    for ty in variants.iter().flat_map(|v| &v.payload) {
        assert_eq!(ty.owner(), HirOwner::Declaration);
        assert!(facts.typed.type_table.type_ref(*ty).is_some());
    }
    let DeclarationId::Definition(id) = &facts.declarations.variant(variants[2].id).unwrap().id
    else {
        panic!("variant identity")
    };
    let hole = facts.aggregates.variant(id).unwrap();
    assert_eq!(hole.payload[0], TypeId::Error);
    assert!(matches!(hole.payload[1], TypeId::Struct(_)));
    assert_eq!(analysis.definition_at(point_offset).unwrap().name, "Point");
    assert_eq!(
        analysis.type_at(text.rfind("p.x").unwrap() + 2),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert!(analysis.result().clone().into_codegen().is_err());
}

#[test]
fn body_edits_rebase_payload_references_and_match_fresh_facts() {
    let text = "fn first() -> i32 { 1 } enum Event { Data(i32, [String]) } fn keep() -> i32 { 7 }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_facts = old.file(file).unwrap().result().facts();
    let old_ty = old_facts.lowered.module.enums[0].variants[0].payload[0];
    sources
        .set(
            "edit.kgr",
            text.replace("{ 1 }", "{ val x: i32 = 2; x }"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_file = new.file(file).unwrap();
    assert!(new_file.signatures_reused());
    let new_facts = new_file.result().facts();
    assert!(new_facts.typed.type_table.type_ref(old_ty).is_none());
    let fresh = analyze(&mut AnalysisDatabase::default(), &sources);
    let fresh = fresh.file(file).unwrap().result().facts();
    new_facts.typed.type_table.assert_same_source_facts(
        &fresh.typed.type_table,
        new_facts.lowered.module.body.arena(),
        fresh.lowered.module.body.arena(),
    );
    assert_eq!(new_facts.aggregates, fresh.aggregates);
    assert!(old_facts.aggregates.same_contracts(&new_facts.aggregates));
}

#[test]
fn imported_payload_changes_invalidate_consumers_and_keep_nominal_owners() {
    let mut sources = SourceDatabase::default();
    let mut insert = |name: &str, text: &str| {
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
        sources.set(&path, text.into(), SourceLayer::Base).unwrap()
    };
    let left = insert(
        "left",
        "pub struct Point { val x: i32 } pub enum Event { Data(Point, i32) }",
    );
    let right = insert(
        "right",
        "pub struct Point { val x: i32 } pub enum Event { Data(Point, i32) }",
    );
    let root = insert(
        "root",
        "use pkg::left; use pkg::right; enum Both { Left(left::Event), Right(right::Event) } fn keep() -> i32 { 7 }",
    );
    let unrelated = insert("unrelated", "pub enum Hidden { A }");
    let mut db = AnalysisDatabase::default();
    let old = analyze(&mut db, &sources);
    let old_root = old.file(root).unwrap();
    assert!(
        old_root.result().diagnostics().is_empty(),
        "{:?}",
        old_root.result().diagnostics()
    );
    let catalog = &old_root.result().facts().aggregates;
    assert_eq!(catalog.enumerations().count(), 3);
    let a = old
        .file(left)
        .unwrap()
        .result()
        .facts()
        .aggregates
        .enumerations()
        .next()
        .unwrap();
    let b = old
        .file(right)
        .unwrap()
        .result()
        .facts()
        .aggregates
        .enumerations()
        .next()
        .unwrap();
    assert_ne!(a.variants[0].payload[0], b.variants[0].payload[0]);
    assert!(catalog.enumeration(&a.id).is_some());
    let hidden = old
        .file(unrelated)
        .unwrap()
        .result()
        .facts()
        .aggregates
        .enumerations()
        .next()
        .unwrap();
    assert!(catalog.enumeration(&hidden.id).is_none());
    sources
        .set(
            "mem://left",
            "pub struct Point { val x: i32 } pub enum Event { Data(Point, String) }".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    let new_root = new.file(root).unwrap();
    assert!(!Arc::ptr_eq(old_root, new_root));
    assert!(!catalog.same_contracts(&new_root.result().facts().aggregates));
    assert_eq!(new_root.result().facts().typed.reused_bodies, 0);
    assert_eq!(
        catalog.enumeration(&a.id).unwrap().variants[0].payload[1],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        new_root
            .result()
            .facts()
            .aggregates
            .enumeration(&a.id)
            .unwrap()
            .variants[0]
            .payload[1],
        TypeId::Builtin(BuiltinType::String)
    );
}
