use kagari_common::DiagnosticKind;

use crate::{
    hir::StmtKind,
    resolver::{ResolvedName, resolve_names},
    tests::common,
};

#[test]
fn reports_duplicate_function_names() {
    let lowered = common::lower_ok(
        r#"
fn foo() {}
fn foo() {}
"#,
    );

    let diagnostics = resolve_names(&lowered).expect_err("resolver should reject duplicates");

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

#[test]
fn resolves_params_and_locals_in_function_body() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { let next: i32 = value; next }");
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let (let_local, init_expr) = match &let_stmt.kind {
        StmtKind::Let {
            local, initializer, ..
        } => (*local, *initializer),
        other => panic!("unexpected stmt kind: {other:?}"),
    };

    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        resolved.expr_resolution(init_expr),
        Some(ResolvedName::Param(function.params[0].id))
    );
    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Local(let_local))
    );
}
