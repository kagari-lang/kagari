use crate::{ast::expr::BinaryOp, ast::views::ExprView, tests::common};

#[test]
fn parses_call_field_and_index_chain() {
    let module = common::parse_ok("fn main() -> i32 { foo(1, 2).bar[0] }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected body");
    let expr = body.tail_expr().expect("expected tail expr");

    match expr {
        ExprView::IndexExpr(index) => {
            match index.receiver().expect("expected receiver") {
                ExprView::FieldExpr(field) => {
                    assert_eq!(field.name_text().as_deref(), Some("bar"));
                    match field.receiver().expect("expected field receiver") {
                        ExprView::CallExpr(call) => {
                            match call.callee().expect("expected callee") {
                                ExprView::PathExpr(path) => {
                                    assert_eq!(path.name_text().as_deref(), Some("foo"))
                                }
                                other => panic!("unexpected callee: {other:?}"),
                            }

                            let args: Vec<_> = call.args().collect();
                            assert_eq!(args.len(), 2);
                            match &args[0] {
                                ExprView::Literal(literal) => {
                                    assert_eq!(literal.text().as_deref(), Some("1"))
                                }
                                other => panic!("unexpected arg0: {other:?}"),
                            }
                            match &args[1] {
                                ExprView::Literal(literal) => {
                                    assert_eq!(literal.text().as_deref(), Some("2"))
                                }
                                other => panic!("unexpected arg1: {other:?}"),
                            }
                        }
                        other => panic!("unexpected field receiver: {other:?}"),
                    }
                }
                other => panic!("unexpected index receiver: {other:?}"),
            }

            match index.index().expect("expected index expr") {
                ExprView::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("0")),
                other => panic!("unexpected index expr: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expr: {other:?}"),
    }
}

#[test]
fn postfix_binds_tighter_than_binary_operators() {
    let module = common::parse_ok("fn main() -> i32 { foo(1) + bar[0] }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected body");
    let expr = body.tail_expr().expect("expected tail expr");

    match expr {
        ExprView::BinaryExpr(binary) => {
            assert_eq!(binary.operator(), Some(BinaryOp::Add));
            assert!(matches!(binary.lhs(), Some(ExprView::CallExpr(_))));
            assert!(matches!(binary.rhs(), Some(ExprView::IndexExpr(_))));
        }
        other => panic!("unexpected top-level expr: {other:?}"),
    }
}
