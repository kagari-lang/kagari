use kagari_common::{DiagnosticKind, TypePosition};

use crate::{resolver::resolve_names, tests::common, typeck::check_module};

#[test]
fn reports_unknown_parameter_type() {
    let module = common::parse_ok("fn foo(value: number) {}");
    let names = resolve_names(&module).expect("resolver should succeed");

    let diagnostics = check_module(&module, &names).expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Parameter,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown parameter type `number` in function `foo` at 7..20"
    );
}

#[test]
fn reports_unknown_return_type() {
    let module = common::parse_ok("fn foo() -> number {}");
    let names = resolve_names(&module).expect("resolver should succeed");

    let diagnostics = check_module(&module, &names).expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Return,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown return type `number` in function `foo` at 11..18"
    );
}
