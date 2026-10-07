use crate::{declarations::DeclarationId, imports::tests::insert, tests::test_analysis};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};
use std::sync::Arc;

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
    let unrelated = insert(&mut sources, "unrelated", "fn value() -> i32 { 9 }");
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
    assert!(result.source_function_at(offset).is_none());
    assert!(incremental.source_target_at(root, offset).is_none());
    assert!(
        incremental
            .check_program(root, &Default::default())
            .is_err()
    );
    assert!(Arc::ptr_eq(
        first.file(unrelated).unwrap(),
        incremental.file(unrelated).unwrap()
    ));
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    assert!(
        first
            .file(root)
            .unwrap()
            .source_function_at(offset)
            .is_some()
    );
}

#[test]
fn alias_target_swap_invalidates_body_reuse_after_trivia_edit() {
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
        .definition_at(root, text.find("m::selected").unwrap() + 3)
        .unwrap();
    let new_offset = "// trivia edit\n".len() + text.find("m::selected").unwrap() + 3;
    let new_type = incremental.definition_at(root, new_offset).unwrap();
    assert_eq!(old_type.location, new_type.location);
    assert_eq!(
        new_type.location,
        fresh.definition_at(root, new_offset).unwrap().location
    );
    assert_eq!(
        result.diagnostics(),
        fresh.file(root).unwrap().result().diagnostics()
    );
    assert!(result.diagnostics().iter().any(|d| matches!(&d.kind, DiagnosticKind::ReturnTypeMismatch { expected, found, .. } if expected == "i32" && found == "bool")));
    assert_eq!(result.facts().typed.reused_bodies, 0);
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
    let old_type = first.file(root).unwrap().type_at(offset).unwrap();
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
    assert_eq!(
        result.type_at(offset),
        fresh.file(root).unwrap().type_at(offset)
    );
    assert_ne!(result.type_at(offset), Some(old_type.clone()));
    assert!(!result.reused());
    assert_eq!(first.file(root).unwrap().type_at(offset), Some(old_type));
}

#[test]
fn alias_target_swap_invalidates_single_function_body_reuse() {
    let mut sources = SourceDatabase::default();
    insert(
        &mut sources,
        "library",
        "pub fn number() -> i32 { 1 } pub fn flag() -> bool { true }",
    );
    insert(
        &mut sources,
        "exports",
        "pub use pkg::library::{number as selected, flag as spare};",
    );
    insert(&mut sources, "facade", "pub use pkg::exports::*;");
    let root = insert(
        &mut sources,
        "root",
        "use pkg::facade as m; fn main() -> i32 { m::selected() }",
    );
    let mut analysis = test_analysis();
    let declarations = analysis
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let file = declarations.file(root).unwrap();
    let declaration = file
        .declarations()
        .iter()
        .find(|d| d.name == "main")
        .unwrap();
    let DeclarationId::Definition(id) = declaration.id else {
        panic!("function definition");
    };
    let owner = file
        .declarations()
        .definitions()
        .resolve(id)
        .unwrap()
        .to_path();
    let first = analysis
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert!(first.diagnostics().is_empty());
    sources
        .set(
            "mem://exports",
            "pub use pkg::library::{flag as selected, number as spare};".into(),
            SourceLayer::Overlay,
        )
        .unwrap();
    let incremental = analysis
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    let fresh = test_analysis()
        .body(sources.snapshot(), &owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(incremental.diagnostics(), fresh.diagnostics());
    assert!(
        incremental
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ReturnTypeMismatch { .. }))
    );
    assert_eq!(incremental.reused_bodies(), 0);
    assert!(first.diagnostics().is_empty());
}

#[test]
fn local_nominal_namespaces_allow_arena_remapping_after_body_edits() {
    let mut sources = SourceDatabase::default();
    let text = "enum Choice { A, B } struct P { val n: i32 } fn changed() -> i32 { 1 } fn keep(p: P) -> i32 { p.n }";
    let root = insert(&mut sources, "root", text);
    let mut analysis = test_analysis();
    let first = analysis
        .snapshot(sources.snapshot(), &Default::default())
        .unwrap();
    assert!(first.file(root).unwrap().result().diagnostics().is_empty());
    sources
        .set(
            "mem://root",
            text.replace("{ 1 }", "{ val shifted: i32 = 2; shifted }"),
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
    assert!(result.signatures_reused());
    assert_eq!(result.result().facts().typed.reused_bodies, 1);
    assert_eq!(result.result().facts().typed.checked_bodies, 1);
}
