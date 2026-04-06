use crate::{
    ast::{Expr, Stmt},
    tests::common,
};

#[test]
fn parses_if_expression_with_else_block() {
    let module = common::parse_ok("fn main() -> i32 { if cond { 1 } else { 2 } }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected function body");
    let expr = body.tail_expr().expect("expected tail expression");

    match expr {
        Expr::IfExpr(if_expr) => {
            match if_expr.condition().expect("expected condition") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("cond")),
                other => panic!("unexpected if condition: {other:?}"),
            }

            let then_branch = if_expr.then_branch().expect("expected then branch");
            match then_branch
                .tail_expr()
                .expect("expected then tail expression")
            {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected then tail expression: {other:?}"),
            }

            match if_expr.else_branch().expect("expected else branch") {
                Expr::BlockExpr(block) => match block.tail_expr().expect("expected else tail") {
                    Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("2")),
                    other => panic!("unexpected else tail expression: {other:?}"),
                },
                other => panic!("unexpected else branch: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expression: {other:?}"),
    }
}

#[test]
fn parses_while_loop_break_and_continue_statements() {
    let module = common::parse_ok("fn main() { while flag { break ; } loop { continue ; } }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    assert_eq!(statements.len(), 2);

    match &statements[0] {
        Stmt::WhileStmt(stmt) => {
            match stmt.condition().expect("expected while condition") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("flag")),
                other => panic!("unexpected while condition: {other:?}"),
            }

            let body = stmt.body().expect("expected while body");
            let inner: Vec<_> = body.statements().collect();
            assert!(matches!(inner.first(), Some(Stmt::BreakStmt(_))));
        }
        other => panic!("unexpected first statement: {other:?}"),
    }

    match &statements[1] {
        Stmt::LoopStmt(stmt) => {
            let body = stmt.body().expect("expected loop body");
            let inner: Vec<_> = body.statements().collect();
            assert!(matches!(inner.first(), Some(Stmt::ContinueStmt(_))));
        }
        other => panic!("unexpected second statement: {other:?}"),
    }
}
