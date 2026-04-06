use crate::tests::common;

#[test]
fn parses_a_function_into_a_syntax_tree() {
    let module = common::parse_ok("fn add(lhs: int, rhs: int) -> int { rhs }");
    let function = common::first_function(&module);

    assert_eq!(function.name_text().as_deref(), Some("add"));
    assert_eq!(
        function
            .param_list()
            .expect("expected parameter list")
            .params()
            .count(),
        2
    );
    assert_eq!(
        function
            .return_type()
            .and_then(|ty| ty.name_text())
            .as_deref(),
        Some("int")
    );
    assert!(function.body().is_some());
    assert_eq!(module.items().count(), 1);
}
