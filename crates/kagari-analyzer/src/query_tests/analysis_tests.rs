use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::cancellation::*;
use kagari_hir::{hir::ids::*, resolver::resolved::*, types::*};

use kagari_source::source_database::{SourceDatabase, SourceLayer};
use kagari_types::scalar::BuiltinType;

#[test]
fn broken_body_preserves_neighbor_and_member_receiver() {
    let mut sources = SourceDatabase::default();
    let text = "struct P { var n: i32 } fn bad() { val p = P { n: 1 }; p. } fn good() -> i32 { val answer = 42; answer }";
    let file = sources
        .set("a.kgr", text.into(), SourceLayer::Base)
        .unwrap();
    let snapshot = test_analysis()
        .snapshot(sources.snapshot(), &CancellationToken::default())
        .unwrap();
    let facts = snapshot.file(file).unwrap();
    assert!(!facts.result().diagnostics().is_empty());
    assert_eq!(
        facts
            .queries()
            .member_receiver_type(text.find("p. }").unwrap() + 2),
        Some(TypeId::Struct(kagari_hir::types::NominalType {
            associated_types: Default::default(),
            declaration: *facts
                .result()
                .facts()
                .declarations
                .definition(ResolvedName::Struct(StructId::new(0)))
                .unwrap(),
            arguments: Vec::new(),
        }))
    );
    let offset = text.rfind("answer").unwrap();
    assert_eq!(
        facts.queries().type_at(offset),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        facts
            .queries()
            .visible_bindings(offset)
            .iter()
            .map(|b| b.declaration.name.as_str())
            .collect::<Vec<_>>(),
        vec!["answer"]
    );
    assert!(facts.result().clone().into_codegen().is_err());
}

#[test]
fn local_function_signature_edits_refresh_facts_and_retain_old_navigation() {
    let mut sources = SourceDatabase::default();
    let original = "fn outer() -> i32 { fn inner() -> i32 { 42 } inner() }";
    let file = sources
        .set("local.kgr", original.into(), SourceLayer::Base)
        .unwrap();
    let mut db = test_analysis();
    let first = db
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(file).unwrap().result().diagnostics().is_empty());
    let call = original.rfind("inner()").unwrap();
    let old_site = first
        .file(file)
        .unwrap()
        .queries()
        .definition_at(call)
        .unwrap()
        .location;
    for changed in [
        "fn outer() -> i32 { fn inner() -> bool { true } inner() }",
        "fn outer() -> i32 { fn renamed() -> i32 { 42 } inner() }",
        "fn outer() -> i32 { fn inner() -> i32 { true } inner() }",
    ] {
        sources
            .set("local.kgr", changed.into(), SourceLayer::Overlay)
            .unwrap();
        let next = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let result = next.file(file).unwrap().result();
        assert!(!result.diagnostics().is_empty(), "{changed}");
        assert_eq!(result.facts().typed.reused_bodies(), 0);
        assert!(result.clone().into_codegen().is_err());
    }
    assert_eq!(
        first
            .file(file)
            .unwrap()
            .queries()
            .definition_at(call)
            .unwrap()
            .location,
        old_site
    );
    assert_eq!(
        first.file(file).unwrap().queries().type_at(call),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn local_function_navigation_distinguishes_shadowed_blocks_and_moved_sources() {
    let source = "fn outer() -> i32 { fn same() -> i32 { 1 } val first = same(); { fn same() -> i32 { 2 } same(); } same() }";
    let mut sources = SourceDatabase::default();
    let mut db = test_analysis();
    let mut retained = Vec::new();
    for prefix in ["", "// moved 😀\r\n"] {
        let text = format!("{prefix}{source}");
        let file = sources
            .set("local-navigation.kgr", text.clone(), SourceLayer::Overlay)
            .unwrap();
        let snapshot = db
            .snapshot(sources.snapshot(), &Default::default())
            .unwrap();
        let facts = snapshot.file(file).unwrap();
        assert!(facts.result().diagnostics().is_empty());
        let first = text.find("fn same").unwrap() + 3;
        let second = text.rfind("fn same").unwrap() + 3;
        for (call, expected) in [
            (text.find("= same()").unwrap() + 2, first),
            (text.find("same(); }").unwrap(), second),
            (text.rfind("same()").unwrap(), first),
        ] {
            assert_eq!(
                facts
                    .queries()
                    .definition_at(call)
                    .unwrap()
                    .location
                    .range
                    .start,
                expected
            );
            let visible = facts.queries().visible_bindings(call);
            let named = visible
                .iter()
                .filter(|binding| binding.declaration.name == "same")
                .collect::<Vec<_>>();
            assert_eq!(named.len(), 1);
            assert_eq!(named[0].declaration.location.range.start, expected);
        }
        retained.push((snapshot, file, text.rfind("same()").unwrap(), first));
    }
    for (snapshot, file, call, expected) in retained {
        assert_eq!(
            snapshot
                .file(file)
                .unwrap()
                .queries()
                .definition_at(call)
                .unwrap()
                .location
                .range
                .start,
            expected
        );
    }
}
