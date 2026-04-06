use kagari_common::DiagnosticKind;

use crate::ast::Expr;
use crate::tests::common;

#[test]
fn parses_top_level_statements_alongside_items() {
    let module = common::parse_ok(
        r#"
let boot = 1;

fn main() -> i32 {
    1
}
"#,
    );

    let statements: Vec<_> = module.statements().collect();
    let items: Vec<_> = module.items().collect();

    assert_eq!(statements.len(), 1);
    assert_eq!(items.len(), 1);
}

#[test]
fn parses_top_level_tail_expression_as_module_result() {
    let module = common::parse_ok(
        r#"
let boot = 1;

boot + 1
"#,
    );

    let statements: Vec<_> = module.statements().collect();
    let tail_expr = module.tail_expr().expect("expected top-level tail expr");

    assert_eq!(statements.len(), 1);
    assert!(matches!(tail_expr, Expr::BinaryExpr(_)));
}

#[test]
fn reports_top_level_control_flow_keywords() {
    let parse = common::parse("return;");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::TopLevelControlFlowNotAllowed
    );
}
