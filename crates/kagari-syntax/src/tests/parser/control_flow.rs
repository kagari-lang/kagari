use crate::{
    ast::{views::ExprView, views::StmtView},
    tests::common,
};

#[test]
fn parses_if_expression_with_else_block() {
    let module = common::parse_ok("fn main() -> i32 { if cond { 1 } else { 2 } }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected function body");
    let expr = body.tail_expr().expect("expected tail expression");

    match expr {
        ExprView::IfExpr(if_expr) => {
            match if_expr.condition().expect("expected condition") {
                ExprView::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("cond")),
                other => panic!("unexpected if condition: {other:?}"),
            }

            let then_branch = if_expr.then_branch().expect("expected then branch");
            match then_branch
                .tail_expr()
                .expect("expected then tail expression")
            {
                ExprView::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected then tail expression: {other:?}"),
            }

            match if_expr.else_branch().expect("expected else branch") {
                ExprView::BlockExpr(block) => {
                    match block.tail_expr().expect("expected else tail") {
                        ExprView::Literal(literal) => {
                            assert_eq!(literal.text().as_deref(), Some("2"))
                        }
                        other => panic!("unexpected else tail expression: {other:?}"),
                    }
                }
                other => panic!("unexpected else branch: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expression: {other:?}"),
    }
}

#[test]
fn parses_while_loop_break_and_continue_statements() {
    for condition in ["flag", "{ check(); flag }", "val Some(value) = input"] {
        let module = common::parse_ok(&format!(
            "fn main() {{ while {condition} {{ break ; }} loop {{ continue ; }} }}"
        ));
        let function = common::first_function(&module);

        let body = function.body().expect("expected function body");
        let statements: Vec<_> = body.statements().collect();

        assert_eq!(statements.len(), 2);

        match &statements[0] {
            StmtView::WhileStmt(stmt) => {
                if let Some(binding) = stmt.binding_condition() {
                    assert!(
                        matches!(binding.initializer(), Some(ExprView::PathExpr(path)) if path.name_text().as_deref() == Some("input"))
                    );
                } else {
                    match stmt.condition().expect("expected while condition") {
                        ExprView::PathExpr(path) => {
                            assert_eq!(path.name_text().as_deref(), Some("flag"))
                        }
                        ExprView::BlockExpr(block) => {
                            assert!(
                                matches!(block.tail_expr(), Some(ExprView::PathExpr(path)) if path.name_text().as_deref() == Some("flag"))
                            );
                        }
                        other => panic!("unexpected while condition: {other:?}"),
                    }
                }

                let body = stmt.body().expect("expected while body");
                let inner: Vec<_> = body.statements().collect();
                assert!(matches!(inner.first(), Some(StmtView::BreakStmt(_))));
            }
            other => panic!("unexpected first statement: {other:?}"),
        }

        match &statements[1] {
            StmtView::LoopStmt(stmt) => {
                let body = stmt.body().expect("expected loop body");
                let inner: Vec<_> = body.statements().collect();
                assert!(matches!(inner.first(), Some(StmtView::ContinueStmt(_))));
            }
            other => panic!("unexpected second statement: {other:?}"),
        }
    }
}
