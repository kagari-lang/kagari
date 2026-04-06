use crate::{ast::Expr, tests::common};

#[test]
fn parses_match_expression_with_simple_patterns() {
    let module = common::parse_ok("fn main() { match color { Red => 1, Green => 2, _ => 3 } }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::MatchExpr(match_expr) => {
            match match_expr.scrutinee().expect("expected scrutinee") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("color")),
                other => panic!("unexpected scrutinee: {other:?}"),
            }

            let arms = match_expr
                .arms()
                .expect("expected arms")
                .arms()
                .collect::<Vec<_>>();

            assert_eq!(arms.len(), 3);

            let first_pattern = arms[0].pattern().expect("expected first pattern");
            assert_eq!(
                first_pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Red")
            );
            match arms[0].expr().expect("expected first expr") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected first arm expr: {other:?}"),
            }

            let second_pattern = arms[1].pattern().expect("expected second pattern");
            assert_eq!(
                second_pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Green")
            );

            let wildcard = arms[2].pattern().expect("expected wildcard pattern");
            assert!(wildcard.is_wildcard());
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}
