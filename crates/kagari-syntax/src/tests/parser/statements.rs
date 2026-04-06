use kagari_common::{DiagnosticKind, Severity};

use crate::{
    ast::{Expr, Stmt},
    tests::common,
};

#[test]
fn parses_let_return_and_tail_expr_in_block() {
    let module =
        common::parse_ok("fn main() -> i32 { let mut value: i32 = 1; value = 2; return; value }");

    let function = common::first_function(&module);

    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    assert_eq!(statements.len(), 3);

    match &statements[0] {
        Stmt::LetStmt(stmt) => {
            assert_eq!(stmt.name_text().as_deref(), Some("value"));
            assert_eq!(
                stmt.ty().and_then(|ty| ty.name_text()).as_deref(),
                Some("i32")
            );
            match stmt.initializer().expect("expected initializer") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected let initializer: {other:?}"),
            }
        }
        other => panic!("unexpected first statement: {other:?}"),
    }

    match &statements[1] {
        Stmt::AssignStmt(stmt) => {
            match stmt.target().expect("expected assignment target") {
                Expr::PathExpr(path) => {
                    assert_eq!(path.name_text().as_deref(), Some("value"));
                }
                other => panic!("unexpected assignment target: {other:?}"),
            }
            match stmt.value().expect("expected assigned value") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("2")),
                other => panic!("unexpected assigned value: {other:?}"),
            }
        }
        other => panic!("unexpected second statement: {other:?}"),
    }

    match &statements[2] {
        Stmt::ReturnStmt(stmt) => assert!(stmt.expr().is_none()),
        other => panic!("unexpected third statement: {other:?}"),
    }

    match body.tail_expr().expect("expected tail expression") {
        Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("value")),
        other => panic!("unexpected tail expr: {other:?}"),
    }
}

#[test]
fn parses_mutable_let_binding() {
    let module = common::parse_ok("fn main() { let mut value = 1; }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    match &statements[0] {
        Stmt::LetStmt(stmt) => {
            assert!(stmt.is_mut());
            assert_eq!(stmt.name_text().as_deref(), Some("value"));
        }
        other => panic!("unexpected first statement: {other:?}"),
    }
}

#[test]
fn parses_field_and_index_assignment_targets() {
    let module = common::parse_ok(
        r#"
fn main() {
    point.inner.x = 1;
    values[0] = 2;
}
"#,
    );
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    match &statements[0] {
        Stmt::AssignStmt(stmt) => match stmt.target().expect("expected target") {
            Expr::FieldExpr(field) => {
                assert_eq!(field.name_text().as_deref(), Some("x"));
            }
            other => panic!("unexpected field assignment target: {other:?}"),
        },
        other => panic!("unexpected first statement: {other:?}"),
    }

    match &statements[1] {
        Stmt::AssignStmt(stmt) => match stmt.target().expect("expected target") {
            Expr::IndexExpr(index) => match index.receiver().expect("expected receiver") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("values")),
                other => panic!("unexpected index receiver: {other:?}"),
            },
            other => panic!("unexpected index assignment target: {other:?}"),
        },
        other => panic!("unexpected second statement: {other:?}"),
    }
}

#[test]
fn reports_missing_let_initializer_operator() {
    let parse = common::parse("fn main() { let value 1; }");

    assert_eq!(parse.diagnostics().len(), 1);
    assert_eq!(parse.diagnostics()[0].severity, Severity::Error);
    assert_eq!(
        parse.diagnostics()[0].kind,
        DiagnosticKind::ExpectedLetInitializer
    );
    assert_eq!(
        parse.diagnostics()[0].to_string(),
        "Error: expected `=` after let binding name at 22..23"
    );
}
