use kagari_source::diagnostic::{DiagnosticKind, Severity};
use rstest::rstest;

use crate::{ast::item::Item, tests::common};

#[rstest]
#[case::missing_closing_paren(
    include_str!("../../../tests/fixtures/ui/parser/reports_missing_closing_paren.kgr"),
    DiagnosticKind::ExpectedFunctionParameterListEnd
)]
#[case::missing_block_end(
    include_str!("../../../tests/fixtures/ui/parser/reports_missing_block_end.kgr"),
    DiagnosticKind::ExpectedBlockEnd
)]
fn reports_missing_delimiter(#[case] input: &str, #[case] kind: DiagnosticKind) {
    // Full producer text, spans, labels and notes belong to the paired UI test.
    let context = "missing delimiter (logical source: test.kg)";
    let parse = common::parse(input);

    let diagnostics = parse.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{context}");
    assert_eq!(diagnostics[0].severity, Severity::Error, "{context}");
    assert_eq!(diagnostics[0].kind, kind, "{context}");
}

#[test]
fn keeps_partial_syntax_tree_when_parse_has_errors() {
    let context = "tests/fixtures/parser/keeps_partial_syntax_tree_when_parse_has_errors.kgr (logical source: test.kg)";
    let parse = common::parse(include_str!(
        "../../../tests/fixtures/parser/keeps_partial_syntax_tree_when_parse_has_errors.kgr"
    ));

    let module = parse.syntax();

    let Some(Item::FnDef(function)) = module.items().next() else {
        panic!("{context}: expected a function item");
    };

    assert_eq!(function.name_text().as_deref(), Some("add"), "{context}");

    assert_eq!(
        function
            .return_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("int"),
        "{context}"
    );

    assert!(function.body().is_some(), "{context}");

    assert_eq!(module.items().count(), 1, "{context}");

    assert_eq!(parse.diagnostics().len(), 1, "{context}");
}
