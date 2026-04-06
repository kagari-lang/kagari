use crate::{ast::Expr, tests::common};

#[test]
fn parses_array_literal_expression() {
    let module = common::parse_ok("fn main() { [1, 2, value] }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::ArrayExpr(array) => {
            let elements = array.elements().collect::<Vec<_>>();
            assert_eq!(elements.len(), 3);
            assert!(matches!(elements[0], Expr::Literal(_)));
            assert!(matches!(elements[1], Expr::Literal(_)));
            assert!(matches!(elements[2], Expr::PathExpr(_)));
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}

#[test]
fn parses_tuple_literal_expression() {
    let module = common::parse_ok("fn main() { (1, value, true) }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::TupleExpr(tuple) => {
            let elements = tuple.elements().collect::<Vec<_>>();
            assert_eq!(elements.len(), 3);
            assert!(matches!(elements[0], Expr::Literal(_)));
            assert!(matches!(elements[1], Expr::PathExpr(_)));
            assert!(matches!(elements[2], Expr::Literal(_)));
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}

#[test]
fn parses_array_and_tuple_types() {
    let module = common::parse_ok("fn main(values: [i32]) -> (i32, string) { (1, \"ok\") }");
    let function = common::first_function(&module);

    let param = function
        .param_list()
        .expect("expected parameter list")
        .params()
        .next()
        .expect("expected parameter");
    let param_type = param.ty().expect("expected parameter type");
    let array_type = param_type.array_type().expect("expected array type");
    assert_eq!(
        array_type
            .element_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("i32")
    );

    let return_type = function.return_type().expect("expected return type");
    let tuple_type = return_type.tuple_type().expect("expected tuple type");
    let elements = tuple_type.element_types().collect::<Vec<_>>();
    assert_eq!(elements.len(), 2);
    assert_eq!(elements[0].name_text().as_deref(), Some("i32"));
    assert_eq!(elements[1].name_text().as_deref(), Some("string"));
}
