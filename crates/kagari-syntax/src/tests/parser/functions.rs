use kagari_source::diagnostic::Severity;

use crate::{
    ast::{borrow::AstNode, views::ExprView, views::StmtView},
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
    assert_eq!(module.text().to_owned(), source);
    let ExprView::ClosureExpr(closure) = function.body().unwrap().tail_expr().unwrap() else {
        panic!("explicit closure");
    };
    assert!(closure.is_async());
    assert_eq!(closure.params().count(), 1);
    let ExprView::BlockExpr(block) = closure.body().unwrap() else {
        panic!("block");
    };
    let ExprView::AwaitExpr(outer) = block.tail_expr().unwrap() else {
        panic!("outer await");
    };
    let ExprView::IndexExpr(index) = outer.expr().unwrap() else {
        panic!("index");
    };
    let ExprView::FieldExpr(field) = index.receiver().unwrap() else {
        panic!("field");
    };
    let ExprView::PropagateExpr(propagation) = field.receiver().unwrap() else {
        panic!("propagation");
    };
    assert!(matches!(propagation.expr(), Some(ExprView::AwaitExpr(_))));

    for text in [
        "fn plain() { async || 1 }",
        "fn plain() { async { 1 } }",
        "async fn run() { async { read().await? }.await; async || {}; async |x| x; }",
        "mod nested { pub(super) async fn run() {} }",
        "struct S {} impl S { pub async fn run(self) {} }",
        "async fn run() { for item in items { item.await; } }",
    ] {
        common::parse_ok(text);
    }
    let module = common::parse_ok("fn plain() { || 1 }");
    let function = common::first_function(&module);
    assert!(!function.is_async());
    let ExprView::ClosureExpr(closure) = function.body().unwrap().tail_expr().unwrap() else {
        panic!("closure");
    };
    assert!(!closure.is_async());
    for text in [
        "fn bad() { async; }",
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
fn async_block_ast_and_recovery_contract() {
    let source = "fn make() { async /* capture */ { async { 42 }.await } }";
    let module = common::parse_ok(source);
    assert_eq!(module.text().to_owned(), source);
    let Some(ExprView::AsyncBlockExpr(block)) =
        common::first_function(&module).body().unwrap().tail_expr()
    else {
        panic!("async block");
    };
    let Some(ExprView::AwaitExpr(wait)) = block.body().unwrap().tail_expr() else {
        panic!("block tail");
    };
    assert!(matches!(wait.expr(), Some(ExprView::AsyncBlockExpr(_))));
    for text in [
        "fn make() { val pending = async; val next = 42; next }",
        "fn make() { val pending = async { val bad = ; }; val next = 42; next }",
    ] {
        let parsed = common::parse(text);
        assert!(!parsed.diagnostics().is_empty());
        assert_eq!(parsed.module().text().to_owned(), text);
        let module = parsed.module();
        let body = common::first_function(&module).body().unwrap();
        assert_eq!(body.statements().count(), 2);
        assert!(matches!(body.tail_expr(), Some(ExprView::PathExpr(_))));
    }
    let parsed = common::parse("fn make() { async {");
    assert!(!parsed.diagnostics().is_empty());
    let module = parsed.module();
    let Some(ExprView::AsyncBlockExpr(block)) =
        common::first_function(&module).body().unwrap().tail_expr()
    else {
        panic!("incomplete async block");
    };
    assert!(block.body().is_some());
}

#[test]
fn parses_a_function_into_a_syntax_tree() {
    let module = common::parse_ok("fn add(lhs: int, rhs: int) -> int { rhs }");
    let function = common::first_function(&module);

    assert_eq!(function.name_text().as_deref(), Some("add"));
    assert_eq!(function.param_list().params().count(), 2);
    assert_eq!(
        function
            .return_type()
            .and_then(common::type_name)
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
fn local_function_syntax_preserves_recovery_and_offsets() {
    let text = "// 中文😀\r\nfn outer() { #[meta(tag = \"local\")] fn inner<T>(x: T) -> T { x } async fn task() {} inner(42) }";
    let module = common::parse_ok(text);
    assert_eq!(module.text().to_owned(), text);
    let body = common::first_function(&module).body().unwrap();
    let statements = body.statements().collect::<Vec<_>>();
    let [StmtView::FnDef(inner), StmtView::FnDef(task)] = statements.as_slice() else {
        panic!("local functions");
    };
    assert_eq!(inner.name_text().as_deref(), Some("inner"));
    assert!(task.is_async());
    assert_eq!(inner.body_span().start, text.find("#[meta").unwrap());
    assert!(matches!(body.tail_expr(), Some(ExprView::CallExpr(_))));
    for text in [
        "fn outer() { fn broken() { val x = ; } fn next() {} next() }",
        "fn outer() { fn broken() fn next() {} next() }",
    ] {
        let parsed = common::parse(text);
        assert!(!parsed.diagnostics().is_empty());
        let module = parsed.module();
        let body = common::first_function(&module).body().unwrap();
        assert!(body.statements().any(
            |stmt| matches!(stmt, StmtView::FnDef(f) if f.name_text().as_deref() == Some("next"))
        ));
        assert!(matches!(body.tail_expr(), Some(ExprView::CallExpr(_))));
    }
}
