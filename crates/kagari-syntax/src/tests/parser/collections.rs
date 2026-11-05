use crate::{
    ast::views::{ExprView, TypeView},
    tests::common,
};

#[test]
fn parses_array_literal_expression() {
    let module = common::parse_ok("fn main() { [1, 2, value] }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        ExprView::ArrayExpr(array) => {
            let elements = array.elements().collect::<Vec<_>>();
            assert_eq!(elements.len(), 3);
            assert!(matches!(elements[0], ExprView::Literal(_)));
            assert!(matches!(elements[1], ExprView::Literal(_)));
            assert!(matches!(elements[2], ExprView::PathExpr(_)));
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
        ExprView::TupleExpr(tuple) => {
            let elements = tuple.elements().collect::<Vec<_>>();
            assert_eq!(elements.len(), 3);
            assert!(matches!(elements[0], ExprView::Literal(_)));
            assert!(matches!(elements[1], ExprView::PathExpr(_)));
            assert!(matches!(elements[2], ExprView::Literal(_)));
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
        .params()
        .next()
        .expect("expected parameter");
    let param_type = param.ty().expect("expected parameter type");
    let TypeView::Array(array_type) = param_type.view() else {
        panic!("array type")
    };
    assert_eq!(
        array_type
            .element_type()
            .and_then(common::type_name)
            .as_deref(),
        Some("i32")
    );

    let return_type = function.return_type().expect("expected return type");
    let TypeView::Tuple(tuple_type) = return_type.view() else {
        panic!("tuple type")
    };
    let elements = tuple_type.element_types().collect::<Vec<_>>();
    assert_eq!(elements.len(), 2);
    assert_eq!(common::type_name(elements[0]).as_deref(), Some("i32"));
    assert_eq!(common::type_name(elements[1]).as_deref(), Some("string"));
}

#[test]
fn distinguishes_grouped_and_singleton_tuple_types() {
    let module = common::parse_ok("fn main(grouped: (i32), singleton: (i32,)) -> () { }");
    let function = common::first_function(&module);
    let params = function.param_list().params().collect::<Vec<_>>();
    let grouped = params[0].ty().unwrap();
    let TypeView::Grouped(grouped) = grouped.view() else {
        panic!("grouped type")
    };
    assert_eq!(
        grouped.ty().and_then(common::type_name).as_deref(),
        Some("i32")
    );
    let TypeView::Tuple(singleton) = params[1].ty().unwrap().view() else {
        panic!("singleton tuple")
    };
    assert_eq!(singleton.element_types().count(), 1);
    let TypeView::Tuple(unit) = function.return_type().unwrap().view() else {
        panic!("unit tuple")
    };
    assert_eq!(unit.element_types().count(), 0);
}
