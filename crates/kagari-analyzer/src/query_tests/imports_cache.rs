use crate::query_tests::imports_tests::insert;
use crate::{AnalyzerQueries, query_tests::test_analysis};
use kagari_common::identity::{map::DefinitionContext, metadata::DefinitionMetadata};
use kagari_hir::analysis::signatures::FileSignatures;
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn transitive_alias_rename_invalidates_unchanged_caller() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "library",
        "pub struct Data {} pub fn value() -> i32 { 1 }",
    );
    insert(
        &mut sources,
        "exports",
        "pub use pkg::library::{Data as old, value as old};",
    );
    insert(&mut sources, "facade", "pub use pkg::exports::*;");
    let text =
        "use pkg::facade as m; fn keep(x: m::old) -> m::old { x } fn main() -> i32 { m::old() }";
    let root = insert(&mut sources, "root", text);

    let offset = text.rfind("old()").unwrap();
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    assert!(
        first
            .file(root)
            .unwrap()
            .queries()
            .source_function_at(offset)
            .is_some()
    );
    sources
        .set(
            "mem://exports",
            "pub use pkg::library::{Data as old, value as new};".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let incremental = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let result = incremental.file(root).unwrap();
    assert_eq!(
        result.result().diagnostics(),
        fresh.file(root).unwrap().result().diagnostics()
    );
    assert!(
        result
            .result()
            .diagnostics()
            .iter()
            .any(|d| matches!(&d.kind, DiagnosticKind::UnknownName { name } if name == "m::old"))
    );
    assert!(result.queries().source_function_at(offset).is_none());
    assert!(
        incremental
            .queries()
            .source_target_at(root, offset)
            .is_none()
    );
    assert!(
        incremental
            .check_program(root, &Default::default())
            .is_err()
    );

    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    assert!(
        first
            .file(root)
            .unwrap()
            .queries()
            .source_function_at(offset)
            .is_some()
    );
}

#[test]
fn alias_target_swap_updates_calls_after_trivia_edit() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "library",
        "pub struct Data {} pub fn number() -> i32 { 1 } pub fn flag() -> bool { true }",
    );
    insert(
        &mut sources,
        "exports",
        "pub use pkg::library::{Data as selected, number as selected, flag as spare};",
    );
    insert(&mut sources, "facade", "pub use pkg::exports::*;");
    let text = "use pkg::facade as m; fn main() -> i32 { val item: m::selected = m::selected {}; m::selected() }";
    let root = insert(&mut sources, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    sources
        .set(
            "mem://exports",
            "pub use pkg::library::{Data as selected, flag as selected, number as spare};".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    sources
        .set(
            "mem://root",
            format!("// trivia edit\n{text}"),
            SourceLayer::Overlay,
        )
        .unwrap();
    let incremental = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh = test_analysis()
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    let result = incremental.file(root).unwrap().result();
    let old_type = first
        .queries()
        .definition_at(root, text.find("m::selected").unwrap() + 3)
        .unwrap();
    let new_offset = "// trivia edit\n".len() + text.find("m::selected").unwrap() + 3;
    let new_type = incremental
        .queries()
        .definition_at(root, new_offset)
        .unwrap();
    assert_eq!(
        old_type.source_location().unwrap(),
        new_type.source_location().unwrap()
    );
    assert_eq!(
        new_type.source_location().unwrap(),
        fresh
            .queries()
            .definition_at(root, new_offset)
            .unwrap()
            .source_location()
            .unwrap()
    );
    assert_eq!(
        result.diagnostics(),
        fresh.file(root).unwrap().result().diagnostics()
    );
    assert!(result.diagnostics().iter().any(|d| matches!(&d.kind, DiagnosticKind::ReturnTypeMismatch { expected, found, .. } if expected == "i32" && found == "bool")));
}

#[test]
fn type_alias_swap_invalidates_signature_reuse() {
    let mut sources = SourceDatabase::default();
    insert(&mut sources, "library", "pub struct A {} pub struct B {}");
    insert(
        &mut sources,
        "exports",
        "pub use pkg::library::{A as Selected, B as Spare};",
    );
    insert(&mut sources, "facade", "pub use pkg::exports::*;");
    let text = "use pkg::facade as m; fn accept(x: m::Selected) {}";
    let root = insert(&mut sources, "root", text);
    let offset = text.find("m::Selected").unwrap();
    let mut analysis = test_analysis();
    let first = analysis
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().diagnostics().is_empty());
    let old_type = first.file(root).unwrap().queries().type_at(offset).unwrap();
    sources
        .set(
            "mem://exports",
            "pub use pkg::library::{B as Selected, A as Spare};".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let incremental = analysis
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh = test_analysis()
        .signatures(sources.snapshot(), &Default::default())
        .unwrap();
    let result = incremental.file(root).unwrap();
    let context = DefinitionContext::new().unwrap();
    let scoped_type = |file: &FileSignatures| {
        DefinitionMetadata::checked(
            file.definitions().clone(),
            file.queries().type_at(offset).unwrap(),
            &Default::default(),
        )
        .unwrap()
        .import_into(&context, &Default::default())
        .unwrap()
        .into_records()
    };
    assert_eq!(scoped_type(result), scoped_type(fresh.file(root).unwrap()));
    assert_ne!(result.queries().type_at(offset), Some(old_type.clone()));
    assert!(!result.reused());
    assert_eq!(
        first.file(root).unwrap().queries().type_at(offset),
        Some(old_type)
    );
}
