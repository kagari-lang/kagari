use crate::{ast::Expr, kind::SyntaxKind, tests::common};

#[test]
fn parses_binary_operator_precedence() {
    let module = common::parse_ok("fn main() -> i32 { 1 + 2 * 3 }");
    let function = common::first_function(&module);

    let body = function.body().expect("expected body");
    let expr = body.tail_expr().expect("expected tail expr");

    match expr {
        Expr::BinaryExpr(binary) => {
            assert_eq!(binary.operator(), Some(SyntaxKind::Plus));

            match binary.lhs().expect("expected lhs") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected lhs: {other:?}"),
            }

            match binary.rhs().expect("expected rhs") {
                Expr::BinaryExpr(rhs) => {
                    assert_eq!(rhs.operator(), Some(SyntaxKind::Star));
                    match rhs.lhs().expect("expected rhs lhs") {
                        Expr::Literal(literal) => {
                            assert_eq!(literal.text().as_deref(), Some("2"))
                        }
                        other => panic!("unexpected rhs lhs: {other:?}"),
                    }
                    match rhs.rhs().expect("expected rhs rhs") {
                        Expr::Literal(literal) => {
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
        Expr::PrefixExpr(prefix) => {
            assert_eq!(prefix.operator(), Some(SyntaxKind::Minus));
            match prefix.expr().expect("expected inner expr") {
                Expr::ParenExpr(paren) => match paren.expr().expect("expected paren expr") {
                    Expr::BinaryExpr(binary) => {
                        assert_eq!(binary.operator(), Some(SyntaxKind::Plus));
                    }
                    other => panic!("unexpected paren expr: {other:?}"),
                },
                other => panic!("unexpected prefix operand: {other:?}"),
            }
        }
        other => panic!("unexpected top-level expr: {other:?}"),
    }
}
