use super::{checked, insert};
use crate::source::{
    lower::{MirLoweringError, lower_to_mir, module_contract},
    program::{SourceProgramError, lower_program_to_mir},
};
use kagari_common::span::Span;
use kagari_source::{diagnostic::DiagnosticKind, source_database::SourceDatabase};

fn unsupported(error: MirLoweringError) -> Option<Span> {
    let MirLoweringError::Diagnostic(diagnostic) = error else {
        panic!("expected an unsupported-static-lowering diagnostic, got {error:?}");
    };
    assert!(
        matches!(diagnostic.kind, DiagnosticKind::UnsupportedSyntax { feature } if feature.contains("static lowering")),
        "{diagnostic:?}"
    );
    diagnostic.span
}

#[test]
fn standalone_lowering_and_contract_projection_reject_statics() {
    for text in [
        "static val UNUSED: i32 = 42; fn run() -> i32 { 7 }",
        "pub static var COUNT: i32 = seed(); fn seed() -> i32 { 0 } fn run() -> i32 { COUNT += 1; COUNT }",
        "static val EMPTY: () = ();",
    ] {
        let mut sources = SourceDatabase::default();
        let root = insert(&mut sources, "root", text);
        let checked = checked(&sources, root);
        let module = checked.root();
        let expected = module
            .lowered
            .source_map
            .static_span(module.lowered.module.statics[0].id);
        assert_eq!(
            unsupported(lower_to_mir(module, &Default::default()).unwrap_err()),
            Some(expected)
        );
        assert_eq!(
            unsupported(module_contract(module, &Default::default()).unwrap_err()),
            Some(expected)
        );
    }

    let mut sources = SourceDatabase::default();
    let root = insert(
        &mut sources,
        "root",
        "pub const VALUE: i32 = 42; fn run() -> i32 { VALUE }",
    );
    let program = checked(&sources, root);
    lower_program_to_mir(&program, &Default::default()).unwrap();

    insert(&mut sources, "sibling", "pub static val UNUSED: i32 = 0;");
    let program = checked(&sources, root);
    // A single-module product does not emit an unreferenced sibling's storage.
    // The checked program retains that sibling and must reject executable emission.
    lower_to_mir(program.root(), &Default::default()).unwrap();
    module_contract(program.root(), &Default::default()).unwrap();
    let error = lower_program_to_mir(&program, &Default::default()).unwrap_err();
    let SourceProgramError::Lowering { module, error } = error else {
        panic!("expected a static lowering error, got {error:?}");
    };
    assert_eq!(module.path, ["sibling"]);
    unsupported(error);
}

#[test]
fn whole_closure_gate_rejects_unused_nested_and_dependency_statics() {
    for (files, owner) in [
        (
            vec![(
                "root",
                "mod nested { mod deeper { static val UNUSED: i32 = 42; } } fn run() -> i32 { 7 }",
            )],
            vec!["root", "nested", "deeper"],
        ),
        (
            vec![
                (
                    "dep",
                    "static var PRIVATE: i32 = 0; pub fn value() -> i32 { 7 }",
                ),
                ("root", "use pkg::dep::value; fn run() -> i32 { value() }"),
            ],
            vec!["dep"],
        ),
        (
            vec![
                ("dep", "mod nested { static val PRIVATE: i32 = 0; }"),
                ("root", "use pkg::dep; fn run() -> i32 { 7 }"),
            ],
            vec!["dep", "nested"],
        ),
        (
            vec![
                ("dep", "pub static var COUNT: i32 = 0;"),
                ("facade", "pub use pkg::dep::COUNT as ALIAS;"),
                (
                    "root",
                    "use pkg::facade::ALIAS; fn run() -> i32 { ALIAS += 1; ALIAS }",
                ),
            ],
            vec!["dep"],
        ),
    ] {
        let mut sources = SourceDatabase::default();
        let mut root = None;
        for (name, text) in files {
            let file = insert(&mut sources, name, text);
            if name == "root" {
                root = Some(file);
            }
        }
        let checked = checked(&sources, root.unwrap());
        if owner == ["root", "nested", "deeper"] {
            // Inline children are separate source modules, not part of this product.
            lower_to_mir(checked.root(), &Default::default()).unwrap();
            module_contract(checked.root(), &Default::default()).unwrap();
        }
        let error = lower_program_to_mir(&checked, &Default::default()).unwrap_err();
        let SourceProgramError::Lowering { module, error } = error else {
            panic!("expected a static lowering error, got {error:?}");
        };
        assert_eq!(module.path, owner);
        let declaration = checked
            .modules()
            .iter()
            .find(|item| item.lowered.source.module_identity() == module.as_ref())
            .unwrap();
        assert_eq!(
            unsupported(error),
            Some(
                declaration
                    .lowered
                    .source_map
                    .static_span(declaration.lowered.module.statics[0].id)
            )
        );
        if owner == ["dep"] && checked.root().lowered.source.text().contains("ALIAS") {
            unsupported(lower_to_mir(checked.root(), &Default::default()).unwrap_err());
            unsupported(module_contract(checked.root(), &Default::default()).unwrap_err());
        }
    }
}
