use crate::{declarations::DeclarationId, imports::tests::insert, tests::test_analysis};
use kagari_source::{
    diagnostic::DiagnosticKind,
    source_database::{SourceDatabase, SourceLayer},
};

#[test]
fn selected_body_follows_the_current_transitive_alias_target() {
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
        .body(sources.snapshot(), &id, &Default::default())
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
        .body(sources.snapshot(), &id, &Default::default())
        .unwrap()
        .unwrap();
    let mut fresh_database = test_analysis();
    let fresh_declarations = fresh_database
        .declarations(sources.snapshot(), &Default::default())
        .unwrap();
    let fresh_owner = fresh_declarations
        .file(root)
        .unwrap()
        .declarations()
        .definitions()
        .lookup(&owner)
        .unwrap();
    let fresh = fresh_database
        .body(sources.snapshot(), &fresh_owner, &Default::default())
        .unwrap()
        .unwrap();
    assert_eq!(incremental.diagnostics(), fresh.diagnostics());
    assert!(
        incremental
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::ReturnTypeMismatch { .. }))
    );
    assert!(first.diagnostics().is_empty());
}

#[test]
fn local_nominal_namespaces_allow_signature_reuse_after_body_edits() {
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
}
