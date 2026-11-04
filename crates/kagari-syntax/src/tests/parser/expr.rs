use crate::{
    ast::expr::{BinaryOp, PrefixOp},
    ast::views::ExprView,
    tests::common,
};

#[test]
fn parses_binary_operator_precedence() {
    let module = common::parse_ok("fn main() -> i32 { 1 + 2 * 3 }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected body");
    let expr = body.tail_expr().expect("expected tail expr");

    match expr {
        ExprView::BinaryExpr(binary) => {
            assert_eq!(binary.operator(), Some(BinaryOp::Add));

            match binary.lhs().expect("expected lhs") {
                ExprView::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected lhs: {other:?}"),
            }

            match binary.rhs().expect("expected rhs") {
                ExprView::BinaryExpr(rhs) => {
                    assert_eq!(rhs.operator(), Some(BinaryOp::Multiply));
                    match rhs.lhs().expect("expected rhs lhs") {
                        ExprView::Literal(literal) => {
                            assert_eq!(literal.text().as_deref(), Some("2"))
                        }
                        other => panic!("unexpected rhs lhs: {other:?}"),
                    }
                    match rhs.rhs().expect("expected rhs rhs") {
                        ExprView::Literal(literal) => {
                            assert_eq!(literal.text().as_deref(), Some("3"))
                        }
                        other => panic!("unexpected rhs rhs: {other:?}"),
                    }
                }
                other => panic!("unexpected rhs: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expr: {other:?}"),
    }
}

#[test]
fn parses_prefix_and_parenthesized_expressions() {
    let module = common::parse_ok("fn main() -> i32 { -(1 + 2) }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected body");
    let expr = body.tail_expr().expect("expected tail expr");

    match expr {
        ExprView::PrefixExpr(prefix) => {
            assert_eq!(prefix.operator(), Some(PrefixOp::Negate));
            match prefix.expr().expect("expected inner expr") {
                ExprView::ParenExpr(paren) => match paren.expr().expect("expected paren expr") {
                    ExprView::BinaryExpr(binary) => {
                        assert_eq!(binary.operator(), Some(BinaryOp::Add));
                    }
                    other => panic!("unexpected paren expr: {other:?}"),
                },
                other => panic!("unexpected prefix operand: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expr: {other:?}"),
    }
}

#[test]
fn propagation_has_postfix_precedence_and_chains_with_member_calls() {
    let module = common::parse_ok("fn f() { -read()?.field[0]?? + 1 }");
    let function = common::first_function(&module);
    let ExprView::BinaryExpr(binary) = function.body().unwrap().tail_expr().unwrap() else {
        panic!("binary");
    };
    let ExprView::PrefixExpr(prefix) = binary.lhs().unwrap() else {
        panic!("prefix");
    };
    let ExprView::PropagateExpr(outer) = prefix.expr().unwrap() else {
        panic!("outer propagation");
    };
    let ExprView::PropagateExpr(inner) = outer.expr().unwrap() else {
        panic!("inner propagation");
    };
    assert!(matches!(inner.expr(), Some(ExprView::IndexExpr(_))));
}
