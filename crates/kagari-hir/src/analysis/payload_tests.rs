use super::*;
use crate::{declarations::DeclarationId, hir::ids::HirOwner};
use kagari_common::identity::{ModuleIdentity, PackageId};
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
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.facts();
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
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.facts();
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
    let authoring_new_facts = new_file.to_unverified(&Default::default()).unwrap();
    let new_facts = authoring_new_facts.facts();
    assert!(new_facts.typed.type_table.type_ref(old_ty).is_none());
    // Recheck user sources without query caches against the same immutable
    // installed source universe; its declaration locations must compare exactly.
    let mut fresh_db = AnalysisDatabase::default();
    fresh_db
        .native_files
        .set(db.native_files.get().unwrap().clone())
        .unwrap();
    let fresh = analyze(&mut fresh_db, &sources);
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.facts();
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
    let authoring_catalog = old_root.to_unverified(&Default::default()).unwrap();
    let catalog = &authoring_catalog.facts().aggregates;
    assert_eq!(
        catalog
            .enumerations()
            .filter(|item| item.id.module.package.0 == "pkg")
            .count(),
        3
    );
    assert_eq!(
        catalog
            .enumerations()
            .filter(|item| item.id.module.package.0 == "kagari-core")
            .count(),
        7
    );
    let authoring_a = old
        .file(left)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let a = authoring_a
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            item.id.module
                == ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["left".into()],
                }
        })
        .unwrap();
    let authoring_b = old
        .file(right)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let b = authoring_b
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            item.id.module
                == ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["right".into()],
                }
        })
        .unwrap();
    assert_ne!(a.variants[0].payload[0], b.variants[0].payload[0]);
    assert!(catalog.enumeration(&a.id).is_some());
    let authoring_hidden = old
        .file(unrelated)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let hidden = authoring_hidden
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            item.id.module
                == ModuleIdentity {
                    package: PackageId("pkg".into()),
                    path: vec!["unrelated".into()],
                }
        })
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
    let authoring_new = new_root.to_unverified(&Default::default()).unwrap();
    assert!(!catalog.same_contracts(&authoring_new.facts().aggregates));
    assert_eq!(new_root.result().facts().typed.reused_bodies, 0);
    assert_eq!(
        catalog.enumeration(&a.id).unwrap().variants[0].payload[1],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        authoring_new
            .facts()
            .aggregates
            .enumeration(&a.id)
            .unwrap()
            .variants[0]
            .payload[1],
        TypeId::Builtin(BuiltinType::String)
    );
}
