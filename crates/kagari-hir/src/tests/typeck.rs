use kagari_common::{DiagnosticKind, TypePosition};

use crate::{
    hir::StmtKind,
    resolver::resolve_names,
    tests::common,
    typeck::check_module,
    types::{BuiltinType, TypeId},
};

#[test]
fn reports_unknown_parameter_type() {
    let lowered = common::lower_ok("fn foo(value: number) {}");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject type");

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
        "Error: unknown parameter type `number` in function `foo` at 13..20"
    );
}

#[test]
fn reports_unknown_return_type() {
    let lowered = common::lower_ok("fn foo() -> number {}");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject type");

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

#[test]
fn records_expression_types_for_resolved_body_expressions() {
    let lowered =
        common::lower_ok("fn main(value: i32) -> i32 { let next: i32 = value + 1; next }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let init_expr = match &let_stmt.kind {
        StmtKind::Let { initializer, .. } => *initializer,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(init_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn reports_return_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { true }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject return");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::ReturnTypeMismatch {
            function_name: "foo".to_string(),
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_break_and_continue_outside_loop() {
    let lowered = common::lower_ok("fn foo() { break; continue; }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject control flow");

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::BreakOutsideLoop);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::ContinueOutsideLoop);
}

#[test]
fn reports_invalid_assignment_target() {
    let lowered = common::lower_ok("fn foo() -> i32 { foo = 1; 0 }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject assignment target");

    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
}

#[test]
fn reports_assignment_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { let x: i32 = 1; x = true; x }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject assignment");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::AssignmentTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_if_branch_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { if true { 1 } else { false } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject if");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::IfBranchTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_match_arm_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { match 1 { 1 => 1, _ => false } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject match");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::MatchArmTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}
