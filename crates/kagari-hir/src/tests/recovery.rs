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
fn where_targets_must_resolve_to_a_generic_parameter() {
    for target in ["Absent", "i32", "P"] {
        let text = format!(
            "struct P {{ val n: i32 }} fn bad<T>(value: T) where {target}: Comparable {{}} fn good() -> i32 {{ 7 }}"
        );
        let analysis = analyze_source(&SourceFile::new("where.kgr", text), Default::default());
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidBoundTarget { name } if name == target)), "{:?}", analysis.diagnostics());
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn impl_where_constraints_are_inherited_without_leaking_through_shadowing() {
    for source in [
        "struct P { val n: i32 } impl<T: HashKey> P { fn count(self, items: Set<T>) -> usize { items.len() } }",
        "struct P { val n: i32 } impl<T> P where T: HashKey { fn count(self, items: Set<T>) -> usize { items.len() } }",
    ] {
        let analysis = analyze_source(&SourceFile::new("bounds.kgr", source), Default::default());
        assert!(
            analysis.diagnostics().is_empty(),
            "{:?}",
            analysis.diagnostics()
        );
        let shadowed = source.replace("fn count(self", "fn count<T>(self");
        let analysis = analyze_source(&SourceFile::new("shadow.kgr", shadowed), Default::default());
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::StandardConstraintNotSatisfied { constraint, .. } if constraint == "HashKey")), "{:?}", analysis.diagnostics());
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn method_where_constraints_do_not_leak_to_sibling_methods() {
    let source = "struct P { val n: i32 } impl<T> P { fn allowed(self, values: Set<T>) -> usize where T: HashKey { values.len() } fn rejected(self, values: Set<T>) -> usize { values.len() } }";
    let analysis = analyze_source(&SourceFile::new("siblings.kgr", source), Default::default());
    let errors = analysis
        .diagnostics()
        .iter()
        .filter(|diagnostic| {
            matches!(
                diagnostic.kind,
                DiagnosticKind::StandardConstraintNotSatisfied { .. }
            )
        })
        .collect::<Vec<_>>();
    assert!(!errors.is_empty(), "{:?}", analysis.diagnostics());
    assert!(
        errors
            .iter()
            .all(|error| error.span.unwrap().start > source.find("fn rejected").unwrap())
    );
}

#[test]
fn inherited_unknown_bounds_are_reported_once_at_the_reference() {
    let source =
        "struct P { val n: i32 } impl<T: Missing> P { fn first(self) {} fn second(self) {} }";
    let analysis = analyze_source(&SourceFile::new("unknown.kgr", source), Default::default());
    assert_eq!(
        analysis.diagnostics().len(),
        1,
        "{:?}",
        analysis.diagnostics()
    );
    assert_eq!(
        analysis.diagnostics()[0].span.unwrap().start,
        source.find("Missing").unwrap()
    );
}

#[test]
fn applied_constraints_are_not_silently_erased() {
    let source = "trait Show<T> { fn show(self); } fn bad<T: Show<i32>>(value: T) {}";
    let analysis = analyze_source(&SourceFile::new("applied.kgr", source), Default::default());
    assert!(!analysis.diagnostics().is_empty());
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
