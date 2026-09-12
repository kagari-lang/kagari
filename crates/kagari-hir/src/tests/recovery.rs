use kagari_common::{DiagnosticKind, SourceFile};

use crate::{
    LanguageFeatureProfile, analyze_source,
    types::{BuiltinType, TypeId},
};

#[test]
fn broken_signatures_preserve_parameter_slots_without_cascading_arity_errors() {
    let analysis = analyze_source(
        &SourceFile::new(
            "bad",
            "fn broken(x: Absent) -> Absent { x } fn good() -> i32 { 7 } fn call() { broken(1); }",
        ),
        LanguageFeatureProfile::default(),
    );
    let broken = analysis
        .facts()
        .typed
        .functions
        .iter()
        .find(|f| f.name == "broken")
        .unwrap();
    assert_eq!(broken.params.len(), 1);
    assert_eq!(broken.params[0].ty, TypeId::Error);
    assert_eq!(broken.return_type, TypeId::Error);
    assert_eq!(
        analysis
            .facts()
            .typed
            .functions
            .iter()
            .find(|f| f.name == "good")
            .unwrap()
            .return_type,
        TypeId::Builtin(BuiltinType::I32)
    );
    assert_eq!(
        analysis.diagnostics().len(),
        2,
        "{:?}",
        analysis.diagnostics()
    );
    assert!(analysis.into_codegen().is_err());
}

#[test]
fn erroneous_annotations_calls_and_indices_never_enter_codegen() {
    for source in [
        "fn main() { let a: Absent = 1; }",
        "struct Bad { let field: Absent; } fn good() {}",
        "fn main() { let a: bool = 1; }",
        "fn main() { 1(); }",
        "fn main() { 1[0]; }",
        "fn main() { [1][true]; }",
        "fn main() { type_of(); }",
    ] {
        let analysis = analyze_source(
            &SourceFile::new("bad", source),
            LanguageFeatureProfile {
                allow_reflection: true,
                ..LanguageFeatureProfile::default()
            },
        );
        assert!(!analysis.diagnostics().is_empty(), "accepted {source}");
        assert!(analysis.into_codegen().is_err(), "accepted {source}");
    }
}

#[test]
fn unresolved_operands_report_the_original_name_error() {
    let analysis = analyze_source(
        &SourceFile::new("bad", "fn main() -> i32 { missing + 1 }"),
        LanguageFeatureProfile::default(),
    );
    assert_eq!(analysis.diagnostics().len(), 1);
    assert!(
        matches!(&analysis.diagnostics()[0].kind, DiagnosticKind::UnknownName { name } if name == "missing")
    );
}
