use crate::{ast::Expr, tests::common};

#[test]
fn explicit_enum_path_preserves_arguments_and_variant_name() {
    use crate::ast::AstNode;
    let text = "fn main() { model::Token<Map<i32, [bool]>>::Empty }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let Expr::PathExpr(path) = common::first_function(&module)
        .body()
        .unwrap()
        .tail_expr()
        .unwrap()
    else {
        panic!("enum path");
    };
    assert_eq!(path.name_text().as_deref(), Some("model::Token::Empty"));
    assert_eq!(path.generic_args().unwrap().args().count(), 1);
}

#[test]
fn explicit_constructor_arguments_preserve_nested_types_and_comparisons() {
    use crate::ast::AstNode;
    let text = "fn main() { model::Marker<Map<i32, [bool]>> { value: 7 } }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let Expr::StructExpr(expr) = common::first_function(&module)
        .body()
        .unwrap()
        .tail_expr()
        .unwrap()
    else {
        panic!("constructor");
    };
    assert_eq!(expr.generic_args().unwrap().args().count(), 1);
    let comparison = common::parse_ok("fn main() { a < b }");
    assert!(matches!(
        common::first_function(&comparison)
            .body()
            .unwrap()
            .tail_expr(),
        Some(Expr::BinaryExpr(_))
    ));
}

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
