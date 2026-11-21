use crate::analysis::*;
use crate::tests::test_analysis;
use kagari_source::source_database::{SourceDatabase, SourceLayer};

fn snapshot(db: &mut AnalysisDatabase, sources: &SourceDatabase) -> AnalysisSnapshot {
    db.snapshot(sources.snapshot(), &Default::default())
        .unwrap()
}

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn native_collection_constructors_infer_items_through_source_aliases() {
    let text = "use std::collections::{HashMap};\nuse alloc::vec::Vec as Sequence; use std::collections::HashMap as Dictionary; fn collect() -> Vec<i32> { Sequence::new() } fn pairs() -> HashMap<i32, bool> { Dictionary::new() }";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("conversion-aliases.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = snapshot(&mut test_analysis(), &sources);
    let file = snapshot.file(id).unwrap();
    assert!(
        file.result().diagnostics().is_empty(),
        "{:?}",
        file.result().diagnostics()
    );
    let authoring_facts = file.to_unverified(&Default::default()).unwrap();
    let facts = authoring_facts.records().facts();
    let call_type = |name: &str| {
        facts
            .lowered
            .module
            .nodes
            .expressions()
            .find_map(|(id, expression)| {
                let ExprKind::Call { callee, .. } = &expression.kind else {
                    return None;
                };
                matches!(&facts.lowered.module.expr(*callee).kind,
                ExprKind::Name { name: actual, .. } if actual == name)
                .then(|| facts.typed.type_table.expr_type(id))
                .flatten()
            })
    };
    assert!(matches!(
        call_type("Sequence::new"),
        Some(TypeId::NativeObject(nominal)) if nominal.arguments == [TypeId::Builtin(BuiltinType::I32)]
    ));
    assert!(matches!(
        call_type("Dictionary::new"),
        Some(TypeId::NativeObject(nominal))
            if nominal.arguments == [TypeId::Builtin(BuiltinType::I32), TypeId::Builtin(BuiltinType::Bool)]
    ));
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn explicit_empty_applications_are_not_erased_to_bare_types() {
    for name in [
        "i32",
        "Point",
        "Mode",
        "View",
        "T",
        "String",
        "RangeFull",
        "Option",
        "Vec",
    ] {
        let text = format!(
            "use core::ops::RangeFull; struct Point {{}} enum Mode {{ Ready }} trait View {{}} fn bad<T>(value: {name}<>) {{}} fn good(x: i32) -> i32 {{ x }}"
        );
        let mut sources = SourceDatabase::default();
        let file = sources
            .set("empty.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let analysis = snapshot(&mut test_analysis(), &sources);
        let file = analysis.file(file).unwrap();
        let authoring_facts = file.to_unverified(&Default::default()).unwrap();
        let facts = authoring_facts.records().facts();
        let start = text.find(&format!("{name}<>")).unwrap();
        let reference = facts
            .lowered
            .module
            .functions
            .iter()
            .find(|f| f.name == "bad")
            .unwrap()
            .params[0]
            .ty;
        assert!(matches!(&facts.lowered.module.type_ref(reference).kind,
            TypeKind::Generic { name: base, args, .. } if base == name && args.is_empty()));
        assert_eq!(file.type_at(start), Some(TypeId::Error), "{name}");
        assert_eq!(
            file.definition_at(start).map(|d| d.name.as_str()),
            (name != "i32").then_some(name)
        );
        assert_eq!(
            file.type_at(text.rfind(" x }").unwrap() + 1),
            Some(TypeId::Builtin(BuiltinType::I32))
        );
        assert!(
            file.result()
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == "KG_TYPE_UNKNOWN_TYPE")
        );
        assert!(file.result().clone().into_codegen().is_err());
    }
}
*/

/* Temporarily paused: stdlib restoration (NC01-NC06).
#[test]
fn type_bindings_shadow_standard_constructors_independently_of_value_bindings() {
    for (name, args) in [
        ("HashMap", "i32, String"),
        ("Set", "i32"),
        ("Option", "i32"),
        ("Result", "i32, String"),
    ] {
        for declaration in [
            format!("struct {name} {{}}"),
            format!("enum {name} {{ Ready }}"),
            format!("trait {name} {{}}"),
            format!("use absent::{name};"),
        ] {
            let text = format!("{declaration} fn bad(x: {name}<{args}>) {{}}");
            let source = SourceFile::new("shadow.kgr", &text);
            let analysis = crate::analyze_source(&source, standard_modules())
                .expect("installed declaration analysis");
            let facts = analysis.facts();
            let function = facts
                .typed
                .signatures
                .functions()
                .iter()
                .find(|f| f.name == "bad")
                .unwrap();
            if declaration.starts_with("use ") && matches!(name, "Option" | "Result") {
                // An unresolved import binds neither namespace; prelude recovery remains available.
                assert!(matches!(function.params[0].ty, TypeId::Enum(_)), "{text}");
            } else {
                assert_eq!(function.params[0].ty, TypeId::Error, "{text}");
            }
            assert!(analysis.into_codegen().is_err(), "{text}");
        }
        let module = match name {
            "HashMap" | "Set" => "std::collections",
            "Option" => "core::option",
            "Result" => "core::result",
            _ => unreachable!(),
        };
        for declaration in [
            format!("fn {name}() {{}}"),
            format!("const {name}: i32 = 1;"),
        ] {
            let text =
                format!("use {module}::{name}; {declaration} fn accept(x: {name}<{args}>) {{}}");
            let analysis = crate::analyze_source(
                &SourceFile::new("independent.kgr", &text),
                standard_modules(),
            )
            .expect("installed declaration analysis");
            assert!(
                analysis.diagnostics().is_empty(),
                "{text}: {:?}",
                analysis.diagnostics()
            );
            assert!(analysis.into_codegen().is_ok(), "{text}");
        }
        let text = format!("fn bad<{name}>(x: {name}<{args}>) {{}}");
        let mut sources = SourceDatabase::default();
        let id = sources
            .set("binder.kgr", text.clone(), SourceLayer::Base)
            .unwrap();
        let analysis = snapshot(&mut test_analysis(), &sources);
        let file = analysis.file(id).unwrap();
        let application = text.find("x: ").unwrap() + 3;
        assert_eq!(file.type_at(application), Some(TypeId::Error));
        assert_eq!(
            file.definition_at(application)
                .unwrap()
                .source_location().unwrap()
                .range
                .start,
            "fn bad<".len()
        );
    }
}
*/

#[test]
fn erroneous_applications_rebase_signature_targets_and_repair_without_stale_errors() {
    let text = "fn before() {} fn bad<T>(x: T<>) {}";
    let mut sources = SourceDatabase::default();
    let id = sources
        .set("edit.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let _old = snapshot(&mut db, &sources);
    let edit = text.replace("fn before() {}", "fn before() { val n = 1; }");
    sources
        .set("edit.kgr", edit.clone(), SourceLayer::Overlay)
        .unwrap();
    let changed = snapshot(&mut db, &sources);
    let file = changed.file(id).unwrap();
    assert!(file.signatures_reused());

    let fresh = snapshot(&mut test_analysis(), &sources);
    file.to_unverified(&Default::default())
        .unwrap()
        .records()
        .facts()
        .typed
        .type_table
        .assert_same_source_facts(
            file.definitions(),
            &fresh
                .file(id)
                .unwrap()
                .to_unverified(&Default::default())
                .unwrap()
                .records()
                .facts()
                .typed
                .type_table,
            fresh.file(id).unwrap().definitions(),
            file.result().facts().lowered.module.nodes.arena(),
            fresh
                .file(id)
                .unwrap()
                .result()
                .facts()
                .lowered
                .module
                .nodes
                .arena(),
        );
    sources
        .set("edit.kgr", edit.replace("T<>", "T"), SourceLayer::Overlay)
        .unwrap();
    let fixed = snapshot(&mut db, &sources);
    assert!(!fixed.file(id).unwrap().signatures_reused());
    assert!(fixed.file(id).unwrap().result().diagnostics().is_empty());
}
