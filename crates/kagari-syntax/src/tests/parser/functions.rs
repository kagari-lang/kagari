use kagari_common::{DiagnosticKind, Severity};

use crate::tests::common;

#[test]
fn parses_a_function_into_a_syntax_tree() {
    let module = common::parse_ok("fn add(lhs: int, rhs: int) -> int { rhs }");
    let function = common::first_function(&module);

    assert_eq!(function.name_text().as_deref(), Some("add"));
    assert_eq!(
        function
            .param_list()
            .expect("expected parameter list")
            .params()
            .count(),
        2
    );
    assert_eq!(
        function
            .return_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("int")
    );
    assert!(function.body().is_some());
    assert_eq!(module.items().count(), 1);
}

#[test]
fn rejects_ref_parameter() {
    let parse = common::parse("fn update(ref value: i32) {}");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidRefParameter
    );
}

#[test]
fn rejects_receiver_modifiers() {
    let parse = common::parse("fn update(mut self: Player) {}");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidReceiverModifier
    );

    let parse = common::parse("fn update(ref self: Player) {}");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidRefParameter
    );
}

#[test]
fn rejects_dyn_trait_type() {
    let parse = common::parse("fn apply(effect: dyn Effect) {}");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::InvalidDynTraitSyntax
    );
}
