use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{ModuleIdentity, PackageId};
use kagari_hir::{analysis::*, types::*};
use kagari_hir::{declarations::DeclarationId, hir::expr::ExprKind};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use kagari_types::{language::binding, scalar::BuiltinType};

fn analyze(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

#[test]
fn native_and_script_variants_share_checked_constructor_and_pattern_facts() {
    let text = "enum Local<T> { Some(T), None } use self::Local::{Some as LocalSome, None as LocalNone}; fn make_option(value:i32)->Option<i32> { Some(value) } fn script(value:i32)->Local<i32> { LocalSome(value) } fn read(value:Option<i32>)->i32 { match value { Some(payload)=>payload, None=>0 } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("variants.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let mut checked = 0;
    for (id, expression) in facts.lowered.module.nodes.expressions() {
        let ExprKind::Call { callee, .. } = &expression.kind else {
            continue;
        };
        let ExprKind::Name { name, .. } = &facts.lowered.module.expr(*callee).kind else {
            continue;
        };
        if !matches!(name.as_str(), "Some" | "LocalSome") {
            continue;
        }
        let constructor = facts.typed.type_table().enum_constructor(id).unwrap();
        let variant = facts
            .aggregates
            .variant(constructor.variant.as_ref().unwrap())
            .unwrap();
        assert_eq!(variant.name, "Some");
        let owner = facts
            .aggregates
            .enumeration(&constructor.enumeration)
            .unwrap();
        assert_eq!(
            Some(owner.id)
                == analysis
                    .definitions()
                    .lookup(&binding::option_declaration()),
            name == "Some"
        );
        let actual = facts.typed.type_table().expr_type(id).unwrap();
        assert_eq!(
            matches!(&actual, TypeId::Enum(nominal) if Some(nominal.declaration) == analysis.definitions().lookup(&binding::option_declaration())),
            name == "Some"
        );
        assert!(matches!(actual, TypeId::Enum(_)));
        checked += 1;
    }
    assert_eq!(checked, 2);
    for needle in ["Some(payload)", "None=>"] {
        let offset = text.find(needle).unwrap();
        let pattern = facts
            .lowered
            .module
            .nodes
            .expressions()
            .find_map(|(_, expression)| {
                let ExprKind::Match { arms, .. } = &expression.kind else {
                    return None;
                };
                arms.iter()
                    .find(|arm| {
                        facts
                            .lowered
                            .module
                            .pattern(arm.pattern)
                            .origin
                            .diagnostic_span()
                            .start
                            == offset
                    })
                    .map(|arm| arm.pattern)
            })
            .unwrap();
        let variant = facts.typed.type_table().pattern_variant(pattern).unwrap();
        let variant = facts.aggregates.variant(variant).unwrap();
        assert_eq!(
            analysis
                .definitions()
                .resolve(variant.owner)
                .unwrap()
                .module()
                .package
                .0,
            "kagari-core"
        );
    }
    let native = snapshot
        .queries()
        .definition_at(file, text.find("Some(value)").unwrap())
        .unwrap();
    let script = snapshot
        .queries()
        .definition_at(file, text.find("LocalSome(value)").unwrap())
        .unwrap();
    assert_ne!(native.id, script.id);
    assert!(native.source_location().is_none());
    assert_eq!(script.source_location().unwrap().file, file);
}

#[test]
fn explicit_variant_imports_shadow_prelude_in_calls_patterns_and_navigation() {
    let text = "enum Local<T> { Some(T), None } use self::Local::{Some, None}; use core::option::Option::{Some as Present, None as Absent}; fn make_option(value:i32)->Option<i32> { Present(value) } fn local(value:i32)->Local<i32> { Some(value) } fn read(value:Local<i32>)->i32 { match value { None=>0, Some(payload)=>payload } } fn read_native(value:Option<i32>)->i32 { match value { Absent=>0, Present(payload)=>payload } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("shadowed-variants.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    for (needle, native) in [
        ("Some(value)", false),
        ("None=>", false),
        ("Some(payload)", false),
        ("Present(value)", true),
        ("Absent=>", true),
        ("Present(payload)", true),
    ] {
        let declaration = snapshot
            .queries()
            .definition_at(file, text.find(needle).unwrap())
            .unwrap();
        assert_eq!(
            declaration.source_location().map(|site| site.file),
            (!native).then_some(file),
            "{needle}"
        );
        let DeclarationId::Definition(id) = &declaration.id else {
            panic!("variant declaration")
        };
        let variant = facts.aggregates.variant(id).unwrap();
        let enumeration = facts.aggregates.enumeration(&variant.owner).unwrap();
        assert_eq!(
            Some(enumeration.id)
                == analysis
                    .definitions()
                    .lookup(&binding::option_declaration()),
            native,
            "{needle}"
        );
    }
    let mut matches = 0;
    for (_, expression) in facts.lowered.module.nodes.expressions() {
        let ExprKind::Match { arms, .. } = &expression.kind else {
            continue;
        };
        for arm in arms {
            assert!(
                facts
                    .typed
                    .type_table()
                    .pattern_variant(arm.pattern)
                    .is_some()
            );
            assert!(
                !facts
                    .names
                    .pattern_is_irrefutable(&facts.lowered.module.nodes, arm.pattern)
            );
            matches += 1;
        }
    }
    assert_eq!(matches, 4);
}

#[test]
fn explicit_enum_navigation_separates_owner_arguments_and_variant_after_errors() {
    let text = "enum Event<T> { Empty, Data(T) } fn good() { Event<bool>::Data(true); } fn wrong() { Event<i32>::Data(false); } fn unknown() { Event<Missing>::Data(missing); }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("explicit-navigation.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let old = analyze(&mut db, &sources);
    assert!(old.check_program(file, &Default::default()).is_err());
    let changed = text.replace("Data(false)", "Data(1)");
    sources
        .set(
            "explicit-navigation.kgr",
            changed.clone(),
            SourceLayer::Base,
        )
        .unwrap();
    let new = analyze(&mut db, &sources);
    assert!(new.check_program(file, &Default::default()).is_err());
    for (snapshot, text) in [(&old, text), (&new, changed.as_str())] {
        let analysis = snapshot.file(file).unwrap();
        let authoring_enumeration = analysis.to_unverified(&Default::default()).unwrap();
        let enumeration = authoring_enumeration
            .records()
            .facts()
            .aggregates
            .enumerations()
            .find(|enumeration| {
                enumeration
                    .declaration
                    .source_location()
                    .is_some_and(|site| site.file == file)
            })
            .unwrap();
        for argument in ["bool", "i32", "Missing"] {
            let start = text.find(&format!("Event<{argument}>")).unwrap();
            assert_eq!(
                analysis.queries().definition_at(start).cloned(),
                Some(enumeration.declaration.clone())
            );
            let variant = start + format!("Event<{argument}>::").len();
            assert_eq!(
                analysis.queries().definition_at(variant).cloned(),
                Some(enumeration.variants[1].declaration.clone())
            );
            let Some(TypeId::Enum(ty)) = analysis.queries().type_at(variant) else {
                panic!("variant type fact");
            };
            assert_eq!(ty.declaration, enumeration.id);
            assert_eq!(ty.arguments.len(), 1);
        }
        assert!(
            analysis
                .queries()
                .definition_at(text.find("Missing").unwrap())
                .is_none()
        );
        assert_eq!(
            analysis.queries().type_at(text.find("bool>").unwrap()),
            Some(TypeId::Builtin(BuiltinType::Bool))
        );
        assert_eq!(
            analysis.queries().type_at(text.find("i32>").unwrap()),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(!analysis.result().diagnostics().is_empty());
    }
    assert_eq!(
        new.file(file)
            .unwrap()
            .result()
            .facts()
            .typed
            .reused_bodies(),
        1
    );
    for (snapshot, has_mismatch) in [(&old, true), (&new, false)] {
        assert_eq!(
            snapshot
                .file(file)
                .unwrap()
                .result()
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(
                    diagnostic.kind,
                    DiagnosticKind::ArgumentTypeMismatch { .. }
                )),
            has_mismatch
        );
    }
}

#[test]
fn constructors_retain_nominal_targets_through_argument_errors() {
    let text = "enum Event { Empty, Data(i32, String) } fn good() -> Event { Event::Data(1, \"ok\") } fn empty() -> Event { Event::Empty } fn wrong() -> Event { Event::Data(true) } fn unknown() -> Event { Event::Absent(missing) }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("constructors.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    let authoring_facts = analysis.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let enumeration = facts
        .aggregates
        .enumerations()
        .find(|enumeration| {
            enumeration
                .declaration
                .source_location()
                .is_some_and(|site| site.file == file)
        })
        .unwrap();
    let good = text.find("Event::Data").unwrap();
    let wrong = text.rfind("Event::Data").unwrap();
    assert_eq!(
        analysis.queries().type_at(good),
        Some(TypeId::Enum(kagari_hir::types::NominalType {
            associated_types: Default::default(),
            declaration: enumeration.id,
            arguments: Vec::new()
        }))
    );
    assert_eq!(
        analysis.queries().type_at(wrong),
        analysis.queries().type_at(good)
    );
    for owner in [good, wrong, text.find("Event::Absent").unwrap()] {
        assert_eq!(
            analysis.queries().definition_at(owner).cloned(),
            Some(enumeration.declaration.clone())
        );
        assert!(
            analysis
                .queries()
                .definition_at(owner + "Event".len())
                .is_none()
        );
    }
    assert_eq!(
        analysis.queries().definition_at(good + "Event::".len()),
        analysis.queries().definition_at(wrong + "Event::".len())
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(good + "Event::".len())
            .cloned(),
        Some(enumeration.variants[1].declaration.clone())
    );
    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("Event::Empty").unwrap() + "Event::".len())
            .cloned(),
        Some(enumeration.variants[0].declaration.clone())
    );
    assert!(
        analysis
            .queries()
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
        .nodes
        .expressions()
        .filter_map(|(id, expr)| {
            matches!(expr.kind, ExprKind::Call { .. })
                .then(|| facts.typed.type_table().enum_constructor(id))
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
fn source_facades_and_value_parameters_preserve_constructor_owners() {
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
    let text = "use pkg::facade; use pkg::right; use pkg::left::Event; fn a() -> Event { facade::Event::Data(1) } fn b() -> right::Event { right::Event::Data(\"ok\") } fn same_name(Event: i32) -> Event { Event::Data(1) }";
    let root = insert("root", text);
    let snapshot = analyze(&mut test_analysis(), &sources);
    let analysis = snapshot.file(root).unwrap();
    let a = analysis
        .queries()
        .definition_at(text.find("facade::Event::Data").unwrap() + "facade::Event::".len())
        .unwrap();
    let b = analysis
        .queries()
        .definition_at(text.find("right::Event::Data").unwrap() + "right::Event::".len())
        .unwrap();
    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("facade::Event::Data").unwrap() + "facade::".len())
            .unwrap()
            .name,
        "Event"
    );
    assert_eq!(a.source_location().unwrap().file, left);
    assert_eq!(b.source_location().unwrap().file, right);
    assert_ne!(a.id, b.id);
    let same_name = text.rfind("Event::Data").unwrap();
    let owner = analysis.queries().definition_at(same_name).unwrap();
    assert_eq!(owner.name, "Event");
    assert_eq!(owner.source_location().unwrap().file, left);
    assert_eq!(
        analysis
            .queries()
            .definition_at(same_name + "Event::".len())
            .unwrap()
            .id,
        a.id
    );
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
}

#[test]
fn struct_update_base_reads_are_resolved_and_captured() {
    let text =
        "struct Item { val value: i32 } fn copy(base: Item) -> fn() -> Item { || Item { ..base } }";
    let mut sources = SourceDatabase::default();
    let file = sources
        .set("update-capture.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = analyze(&mut test_analysis(), &sources);
    let analysis = snapshot.file(file).unwrap();
    assert!(
        analysis.result().diagnostics().is_empty(),
        "{:?}",
        analysis.result().diagnostics()
    );
    let facts = analysis.result().facts();
    let closure = facts
        .lowered
        .module
        .nodes
        .expressions()
        .find(|(_, expr)| matches!(expr.kind, ExprKind::Closure { .. }))
        .unwrap()
        .0;
    assert_eq!(facts.names.closure_captures(closure).len(), 1);

    assert_eq!(
        analysis
            .queries()
            .definition_at(text.find("..base").unwrap() + 2)
            .unwrap()
            .name,
        "base"
    );
    analysis
        .result()
        .clone()
        .into_codegen()
        .expect("captured update base");
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
        .nodes
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
    let _fresh = fresh_db
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();

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
    assert_eq!(
        body.queries().type_at(text.find("(7)").unwrap() + 1),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}
