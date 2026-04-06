use kagari_common::DiagnosticKind;

use crate::{resolver::resolve_names, tests::common};

#[test]
fn reports_duplicate_function_names() {
    let module = common::parse_ok(
        r#"
fn foo() {}
fn foo() {}
"#,
    );

    let diagnostics = resolve_names(&module).expect_err("resolver should reject duplicates");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::DuplicateFunction {
            name: "foo".to_string(),
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: duplicate function `foo` at 13..24"
    );
}
