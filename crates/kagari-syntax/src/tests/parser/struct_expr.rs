use crate::{ast::Expr, tests::common};

#[test]
fn parses_struct_literal_expression() {
    let module = common::parse_ok("fn main() { Player { hp: 10, name: value } }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::StructExpr(struct_expr) => {
            assert_eq!(
                struct_expr
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Player")
            );

            let field_list = struct_expr.field_list().expect("expected field list");
            let fields: Vec<_> = field_list.fields().collect();

            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name_text().as_deref(), Some("hp"));
            match fields[0].value().expect("expected hp value") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("10")),
                other => panic!("unexpected hp value: {other:?}"),
            }

            assert_eq!(fields[1].name_text().as_deref(), Some("name"));
            match fields[1].value().expect("expected name value") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("value")),
                other => panic!("unexpected name value: {other:?}"),
            }
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}
