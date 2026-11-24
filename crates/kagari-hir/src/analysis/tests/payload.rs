use crate::analysis::*;
use crate::types::*;
use crate::{
    aggregates::{AggregateCatalog, EnumSignature, VariantSignature},
    tests::{common::import_facts, test_analysis},
};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::{language::binding, scalar::BuiltinType};
use std::collections::BTreeMap;

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
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
    let authoring_new_facts = new_file.to_unverified(&Default::default()).unwrap();
    let new_facts = authoring_new_facts.records().facts();
    assert!(new_facts.typed.type_table.type_ref(old_ty).is_none());
    let original_enum = old_facts
        .aggregates
        .enumerations()
        .find(|item| {
            item.declaration
                .source_location()
                .is_some_and(|site| site.file == file)
        })
        .unwrap();
    let current_enum = new_facts.aggregates.enumeration(&original_enum.id).unwrap();
    let original_variant = &original_enum.variants[0];
    let current_variant = &current_enum.variants[0];
    assert_eq!(original_variant.id, current_variant.id);
    assert_eq!(original_variant.payload, current_variant.payload);
    assert_eq!(
        old_facts.typed.type_table.type_ref(old_ty).unwrap().ty,
        TypeId::Builtin(BuiltinType::I32)
    );

    // Independent adoption/lowering must agree semantically without sharing native
    // graph provenance. Remap definition IDs, not source addresses or HIR maps.
    let mut independent_db = test_analysis();
    let independent = analyze(&mut independent_db, &sources);
    let independent_file = independent.file(file).unwrap();
    let authoring_independent = independent_file.to_unverified(&Default::default()).unwrap();
    assert_ne!(
        authoring_new_facts.definitions().id(),
        authoring_independent.definitions().id()
    );
    let independent_facts = authoring_independent.records().facts();
    assert_ne!(
        new_facts.lowered.module.nodes.arena(),
        independent_facts.lowered.module.nodes.arena()
    );
    new_facts.typed.type_table.assert_same_source_facts(
        authoring_new_facts.definitions(),
        &independent_facts.typed.type_table,
        authoring_independent.definitions(),
        new_facts.lowered.module.nodes.arena(),
        independent_facts.lowered.module.nodes.arena(),
    );
    let independent_aggregates = import_facts(
        &independent_facts.aggregates,
        authoring_independent.definitions(),
        &db.definitions,
    );
    // Compare enum contracts without navigation sites, and complete independent impls.
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
    for path in [binding::option_declaration(), binding::result_declaration()] {
        let id = old.definitions().lookup(&path).unwrap();
        assert!(catalog.enumeration(&id).is_some(), "{path:?}");
    }
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
    assert_eq!(
        catalog.enumeration(&a.id).unwrap().variants[0].payload,
        a.variants[0].payload
    );
    assert_eq!(
        catalog.enumeration(&b.id).unwrap().variants[0].payload,
        b.variants[0].payload
    );
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
    let authoring_new = new_root.to_unverified(&Default::default()).unwrap();
    let new_catalog = &authoring_new.records().facts().aggregates;
    assert_eq!(
        catalog.enumeration(&a.id).unwrap().variants[0].payload[1],
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        new_catalog.enumeration(&a.id).unwrap().variants[0].payload,
        [
            a.variants[0].payload[0].clone(),
            TypeId::Builtin(BuiltinType::String)
        ]
    );
    assert_eq!(
        new_catalog.enumeration(&b.id).unwrap().variants[0].payload,
        b.variants[0].payload
    );
}
