use kagari_source::diagnostic::{DiagnosticKind, Severity};
use rstest::rstest;

use crate::{ast::item::Item, tests::common};

#[rstest]
#[case::missing_closing_paren(
    "reports_missing_closing_paren.kgr",
    include_str!("../../../tests/fixtures/parser/reports_missing_closing_paren.kgr"),
    DiagnosticKind::ExpectedFunctionParameterListEnd,
    "Error: expected `)` after parameters at 16..17"
)]
#[case::missing_block_end(
    "reports_missing_block_end.kgr",
    include_str!("../../../tests/fixtures/parser/reports_missing_block_end.kgr"),
    DiagnosticKind::ExpectedBlockEnd,
    "Error: expected `}` to end block at 18..18"
)]
fn reports_missing_delimiter(
    #[case] fixture: &str,
    #[case] input: &str,
    #[case] kind: DiagnosticKind,
    #[case] text: &str,
) {
    let context = format!("tests/fixtures/parser/{fixture} (logical source: test.kg)");
    let parse = common::parse(input);

    let diagnostics = parse.diagnostics();
    assert_eq!(diagnostics.len(), 1, "{context}");
    assert_eq!(diagnostics[0].severity, Severity::Error, "{context}");
    assert_eq!(diagnostics[0].kind, kind, "{context}");
    assert_eq!(diagnostics[0].to_string(), text, "{context}");
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
