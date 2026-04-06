use kagari_common::{DiagnosticKind, Severity};

use crate::tests::common;

#[test]
fn reports_missing_closing_paren() {
    let parse = common::parse("fn add(lhs: int { rhs }");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::ExpectedFunctionParameterListEnd
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: expected `)` after parameters at 16..17"
    );
}

#[test]
fn reports_missing_block_end() {
    let parse = common::parse("fn add(lhs: int) {");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::ExpectedBlockEnd
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: expected `}` to end block at 18..18"
    );
}

#[test]
fn keeps_partial_syntax_tree_when_parse_has_errors() {
    let parse = common::parse("fn add(lhs: int) -> int {");
    let module = parse.syntax();

    let function = common::first_function(&module);

    assert_eq!(function.name_text().as_deref(), Some("add"));
    assert_eq!(
        function
            .return_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("int")
    );
    assert!(function.body().is_some());
    assert_eq!(module.items().count(), 1);
    assert_eq!(parse.diagnostics().len(), 1);
}
