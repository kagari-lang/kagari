use crate::{
    ast::{
        borrow::AstNode,
        views::{ExprView, StmtView},
        visit::NodeRef,
    },
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

#[test]
fn conditions_allow_struct_expressions_inside_delimiters() {
    for condition in [
        "check(Point { x: 1 }) && flag",
        "(Point { x: 1 }).x == 1",
        "[Point { x: 1 }][0].x == 1",
        "values[Point { x: 1 }.x] == 1",
        "{ val p = Point { x: 1 }; p.x == 1 }",
        "match point { _ => Point { x: 1 }.x == 1 }",
        "check(|| Point { x: 1 })",
        "check(async { Point { x: 1 } })",
    ] {
        let text = format!("fn main() {{ if {condition} {{ body(); }} }}");
        let module = common::parse_ok(&text);
        let body = common::first_function(&module).body().unwrap();
        let Some(ExprView::IfExpr(conditional)) = body.tail_expr() else {
            panic!("expected conditional: {condition}");
        };
        assert_eq!(conditional.condition().unwrap().source_text(), condition);
        assert_eq!(
            conditional.then_branch().unwrap().source_text(),
            "{ body(); }"
        );
        let mut constructors = 0;
        NodeRef::from(conditional).visit(&mut |node| {
            constructors += usize::from(matches!(node, NodeRef::StructExpr(_)));
            true
        });
        assert_eq!(constructors, 1, "{condition}");
    }
}

#[test]
fn control_flow_delimiters_remain_outside_nested_struct_expressions() {
    for statement in [
        "while check(Point { x: 1 }) { break; }",
        "for value in values(Point { x: 1 }) {}",
        "val result = match (Point { x: 1 }) { _ => 0 };",
        "if val Some(value) = make(Point { x: 1 }) {}",
    ] {
        let text = format!("fn main() {{ {statement} val after = 1; }}");
        let module = common::parse_ok(&text);
        let body = common::first_function(&module).body().unwrap();
        let statements = body.statements().collect::<Vec<_>>();
        assert_eq!(statements.len(), 2, "{statement}");
        assert_eq!(statements[0].source_text(), statement);
        assert!(matches!(statements[1], StmtView::BindingStmt(_)));
    }
}
