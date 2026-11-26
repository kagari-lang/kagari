use crate::analyze_source;
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};

#[test]
fn enum_match_coverage_respects_guard_mutation_through_an_alias() {
    let source = include_str!("../../../../tests/fixtures/typeck/enum-match-guard-alias.kgr");
    let result = analyze_source(&SourceFile::new("alias.kgr", source), vec![]).unwrap();
    let [diagnostic] = result.diagnostics() else {
        panic!("{:?}", result.diagnostics())
    };
    assert_eq!(
        diagnostic.kind,
        DiagnosticKind::NonExhaustiveEnumMatch {
            enum_name: "Event".into(),
            uncovered_variants: vec!["Hit".into()],
        }
    );
    let span = diagnostic.span.unwrap();
    assert_eq!(&source[span.start..span.end], "unstable");
}

#[test]
fn enum_match_coverage_uses_checked_patterns_and_entire_payload_rows() {
    let source = include_str!("../../../../tests/fixtures/typeck/enum-match-coverage.kgr");
    let result = analyze_source(&SourceFile::new("coverage.kgr", source), vec![]).unwrap();
    let mut uncovered = Vec::new();
    for diagnostic in result.diagnostics() {
        let DiagnosticKind::NonExhaustiveEnumMatch {
            enum_name,
            uncovered_variants,
        } = &diagnostic.kind
        else {
            panic!("unexpected diagnostic: {diagnostic}");
        };
        let span = diagnostic.span.unwrap();
        assert_eq!(diagnostic.kind.code(), "KG_TYPE_NON_EXHAUSTIVE_ENUM_MATCH");
        uncovered.push((
            &source[span.start..span.end],
            enum_name.as_str(),
            uncovered_variants
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        ));
    }
    assert_eq!(
        uncovered,
        [
            ("s", "State", vec!["Waiting", "Finished"]),
            ("s", "State", vec!["Waiting"]),
            ("pair", "Message<(bool, bool)>", vec!["Data"]),
            ("gap", "Message<char>", vec!["Data"]),
            ("ends", "Triple", vec!["Fields"]),
        ]
    );
}
