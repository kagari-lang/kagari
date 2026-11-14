use crate::analysis::*;
use crate::{
    aggregates::{AggregateCatalog, EnumSignature, VariantSignature},
    declarations::DeclarationId,
    hir::ids::HirOwner,
    tests::{common::import_facts, test_analysis},
};
use kagari_common::{
    identity::{ModuleIdentity, PackageId},
    span::Span,
};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;
use std::collections::BTreeMap;

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
    let facts = authoring_facts.records().facts();
    let variants = &facts.lowered.module.enums[0].variants;
    assert_eq!(
        variants.iter().map(|v| v.payload.len()).collect::<Vec<_>>(),
        [0, 3, 2]
    );
    for ty in variants.iter().flat_map(|v| &v.payload) {
        assert_eq!(ty.owner(), HirOwner::Declaration);
        assert!(facts.typed.type_table.type_ref(*ty).is_some());
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
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let authoring_old_facts = old
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let old_facts = authoring_old_facts.records().facts();
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
    let new_facts = authoring_new_facts.records().facts();
    assert!(new_facts.typed.type_table.type_ref(old_ty).is_none());
    // Recheck with empty semantic caches but the same adopted native inputs and live
    // native graphs. Exact catalog equality includes their file/arena provenance;
    // sharing the scope avoids copying and remapping lowering-local attachment maps.
    let mut fresh_db = AnalysisDatabase {
        definitions: db.definitions.clone(),
        native_files: db.native_files.clone(),
        ..test_analysis()
    };
    let fresh = analyze(&mut fresh_db, &sources);
    let authoring_fresh = fresh
        .file(file)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let fresh = authoring_fresh.records().facts();
    let fresh_aggregates = import_facts(
        &fresh.aggregates,
        authoring_fresh.definitions(),
        &db.definitions,
    );
    assert_eq!(&new_facts.aggregates, fresh_aggregates.records());
    let fresh_functions = import_facts(
        &fresh.imported_functions,
        authoring_fresh.definitions(),
        &db.definitions,
    );
    assert_eq!(&new_facts.imported_functions, fresh_functions.records());
    let fresh_types = import_facts(
        fresh.declarations.imported_types(),
        authoring_fresh.definitions(),
        &db.definitions,
    );
    assert_eq!(
        new_facts.declarations.imported_types(),
        fresh_types.records()
    );
    assert!(old_facts.aggregates.same_contracts(&new_facts.aggregates));

    // Independent adoption/lowering must agree semantically without sharing native
    // graph provenance. Remap definition IDs, not source addresses or HIR maps.
    let mut independent_db = test_analysis();
    let independent = analyze(&mut independent_db, &sources);
    let independent_file = independent.file(file).unwrap();
    assert!(!independent_file.signatures_reused());
    let authoring_independent = independent_file.to_unverified(&Default::default()).unwrap();
    assert_ne!(
        authoring_new_facts.definitions().id(),
        authoring_independent.definitions().id()
    );
    let independent_facts = authoring_independent.records().facts();
    assert_ne!(
        new_facts.lowered.module.body.arena(),
        independent_facts.lowered.module.body.arena()
    );
    new_facts.typed.type_table.assert_same_source_facts(
        authoring_new_facts.definitions(),
        &independent_facts.typed.type_table,
        authoring_independent.definitions(),
        new_facts.lowered.module.body.arena(),
        independent_facts.lowered.module.body.arena(),
    );
    let independent_aggregates = import_facts(
        &independent_facts.aggregates,
        authoring_independent.definitions(),
        &db.definitions,
    );
    // same_contracts retains exact inherent-method provenance. Compare all enum
    // contracts without navigation sites, and complete source-independent impls.
    let enum_contracts = |catalog: &AggregateCatalog| {
        catalog
            .enumerations()
            .map(|enumeration| {
                let EnumSignature {
                    id,
                    generic_params,
                    bounds,
                    declaration: _,
                    variants,
                } = enumeration;
                let variants = variants
                    .iter()
                    .map(|variant| {
                        let VariantSignature {
                            reports_failure,
                            id,
                            owner,
                            slot,
                            name,
                            payload,
                            declaration: _,
                        } = variant;
                        (
                            *id,
                            *owner,
                            *slot,
                            name.clone(),
                            payload.clone(),
                            *reports_failure,
                        )
                    })
                    .collect::<Vec<_>>();
                (*id, (generic_params.clone(), bounds.clone(), variants))
            })
            .collect::<BTreeMap<_, _>>()
    };
    assert_eq!(
        enum_contracts(&new_facts.aggregates),
        enum_contracts(independent_aggregates.records())
    );
    assert_eq!(
        new_facts.aggregates.implementations().collect::<Vec<_>>(),
        independent_aggregates
            .records()
            .implementations()
            .collect::<Vec<_>>()
    );
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
    sources
        .bind_module(
            "mem://unrelated",
            ModuleIdentity {
                package: PackageId("independent".into()),
                path: vec!["unrelated".into()],
            },
        )
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    let old_root = old.file(root).unwrap();
    assert!(
        old_root.result().diagnostics().is_empty(),
        "{:?}",
        old_root.result().diagnostics()
    );
    let authoring_catalog = old_root.to_unverified(&Default::default()).unwrap();
    let catalog = &authoring_catalog.records().facts().aggregates;
    assert_eq!(
        catalog
            .enumerations()
            .filter(|item| old
                .definitions()
                .resolve(item.id)
                .unwrap()
                .module()
                .package
                .0
                == "pkg")
            .count(),
        3
    );
    assert_eq!(
        catalog
            .enumerations()
            .filter(|item| old
                .definitions()
                .resolve(item.id)
                .unwrap()
                .module()
                .package
                .0
                == "kagari-core")
            .count(),
        10
    );
    let authoring_a = old
        .file(left)
        .unwrap()
        .to_unverified(&Default::default())
        .unwrap();
    let a = authoring_a
        .records()
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            *old.definitions().resolve(item.id).unwrap().module()
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
        .records()
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            *old.definitions().resolve(item.id).unwrap().module()
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
        .records()
        .facts()
        .aggregates
        .enumerations()
        .find(|item| {
            *old.definitions().resolve(item.id).unwrap().module()
                == ModuleIdentity {
                    package: PackageId("independent".into()),
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
    assert!(!catalog.same_contracts(&authoring_new.records().facts().aggregates));
    assert_eq!(new_root.result().facts().typed.reused_bodies, 0);
    assert_eq!(
        catalog.enumeration(&a.id).unwrap().variants[0].payload[1],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        authoring_new
            .records()
            .facts()
            .aggregates
            .enumeration(&a.id)
            .unwrap()
            .variants[0]
            .payload[1],
        TypeId::Builtin(BuiltinType::String)
    );
}
