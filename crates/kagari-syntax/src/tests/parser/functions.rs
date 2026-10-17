use kagari_source::diagnostic::Severity;

use crate::{
    ast::{expr::Expr, traits::AstNode},
    lexer::lex,
    tests::common,
    token::TokenKind,
};

#[test]
fn async_callable_syntax_contract() {
    let source = "#[meta(tag = \"async\")] pub async fn run<T>(x: T) -> T { async |y: T| { y.await?.field[0].await } }";
    let module = common::parse_ok(source);
    let function = common::first_function(&module);
    assert!(function.is_async());
    assert_eq!(module.syntax().text().to_string(), source);
    let Expr::ClosureExpr(closure) = function.body().unwrap().tail_expr().unwrap() else {
        panic!("explicit closure");
    };
    assert!(closure.is_async());
    assert_eq!(closure.params().count(), 1);
    let Expr::BlockExpr(block) = closure.body().unwrap() else {
        panic!("block");
    };
    let Expr::AwaitExpr(outer) = block.tail_expr().unwrap() else {
        panic!("outer await");
    };
    let Expr::IndexExpr(index) = outer.expr().unwrap() else {
        panic!("index");
    };
    let Expr::FieldExpr(field) = index.receiver().unwrap() else {
        panic!("field");
    };
    let Expr::PropagateExpr(propagation) = field.receiver().unwrap() else {
        panic!("propagation");
    };
    assert!(matches!(propagation.expr(), Some(Expr::AwaitExpr(_))));

    for text in [
        "fn plain() { async || 1 }",
        "mod nested { pub(super) async fn run() {} }",
        "struct S {} impl S { pub async fn run(self) {} }",
        "async fn run() { for item in items { item.await; } }",
    ] {
        common::parse_ok(text);
    }
    let module = common::parse_ok("fn plain() { || 1 }");
    let function = common::first_function(&module);
    assert!(!function.is_async());
    let Expr::ClosureExpr(closure) = function.body().unwrap().tail_expr().unwrap() else {
        panic!("closure");
    };
    assert!(!closure.is_async());
    for text in [
        "fn bad() { async { 1 } }",
        "async value() {}",
        "fn async() {}",
        "fn bad() { await value; }",
    ] {
        assert!(!common::parse(text).diagnostics().is_empty(), "{text}");
    }
    assert_eq!(
        lex("async await async_value awaitable")
            .into_iter()
            .filter(|token| token.kind != TokenKind::Whitespace)
            .map(|token| token.kind)
            .collect::<Vec<_>>(),
        [
            TokenKind::AsyncKw,
            TokenKind::AwaitKw,
            TokenKind::Ident,
            TokenKind::Ident,
            TokenKind::Eof
        ]
    );
}

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

#[test]
fn rejects_ref_parameter() {
    let parse = common::parse("fn update(ref value: i32) {}");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );
}

#[test]
fn rejects_receiver_modifiers() {
    let parse = common::parse("fn update(mut self: Player) {}");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );

    let parse = common::parse("fn update(ref self: Player) {}");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );
}

#[test]
fn rejects_dyn_trait_type() {
    let parse = common::parse("fn apply(effect: dyn Effect) {}");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );
}
