//! Lexical declarations and noncapturing callable boundaries.

use crate::{analyze_source, hir::item::function::FunctionKind};
use kagari_source::source::SourceFile;
use kagari_stdlib::catalog;
use rstest::rstest;

#[rstest]
#[case::forward_call(
    "fn outer() -> i32 { val answer = inner(41); fn inner(x: i32) -> i32 { x + 1 } answer }"
)]
#[case::mutual_recursion(
    "fn outer() -> bool { fn even(n: i32) -> bool { if n == 0 { true } else { odd(n - 1) } } fn odd(n: i32) -> bool { if n == 0 { false } else { even(n - 1) } } even(4) }"
)]
#[case::shadowed_generics("fn outer<T>(value: T) -> T { fn id<T>(x: T) -> T { x } id(value) }")]
#[case::contextual_generic_value("fn outer() -> fn(i32) -> i32 { fn id<T>(x: T) -> T { x } id }")]
#[case::explicit_generic_value("fn outer() -> i32 { fn id<T>(x: T) -> T { x } (id::<i32>)(42) }")]
#[case::ordinary_function_value(
    "fn outer() -> fn(i32) -> i32 { fn inc(x: i32) -> i32 { x + 1 } inc }"
)]
#[case::block_shadowing(
    "fn outer() -> i32 { fn f() -> i32 { 1 } val f = || 2; { fn f() -> i32 { 3 } f(); } f() }"
)]
#[case::returned_capture(
    "fn outer() -> fn() -> i32 { fn make(x: i32) -> fn() -> i32 { || x } make(42) }"
)]
#[case::closure_local(
    "fn outer() -> i32 { val closure = || { fn inner() -> i32 { 42 } inner() }; closure() }"
)]
#[case::method_local(
    "struct S {} impl S { fn method(self) -> i32 { fn inner() -> i32 { 42 } inner() } }"
)]
#[case::async_local("async fn outer() -> i32 { async fn inner() -> i32 { 42 } inner().await }")]
#[case::forward_zero_argument_call(
    "fn outer() -> i32 { val answer = inner(); fn inner() -> i32 { 42 } answer }"
)]
fn local_function_scope_and_callable_contracts(#[case] text: &str) {
    let source = SourceFile::new("locals.kgr", text);
    let result = analyze_source(&source, catalog::shared().unwrap()).unwrap();
    assert!(
        result.diagnostics().is_empty(),
        "{text}\n{:?}",
        result.diagnostics()
    );
    let facts = result.facts();
    assert!(
        facts
            .lowered
            .module
            .functions
            .iter()
            .any(|f| matches!(f.kind, FunctionKind::Local { .. }))
    );
    assert_eq!(
        facts.typed.checked_bodies,
        facts.lowered.module.functions.len()
    );
}
