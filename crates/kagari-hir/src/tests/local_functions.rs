//! Lexical declarations and noncapturing callable boundaries.

use crate::{analyze_source, hir::item::function::FunctionKind};
use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_stdlib::catalog;

#[test]
fn local_function_scope_and_callable_contracts() {
    for text in [
        "fn outer() -> i32 { val answer = inner(41); fn inner(x: i32) -> i32 { x + 1 } answer }",
        "fn outer() -> bool { fn even(n: i32) -> bool { if n == 0 { true } else { odd(n - 1) } } fn odd(n: i32) -> bool { if n == 0 { false } else { even(n - 1) } } even(4) }",
        "fn outer<T>(value: T) -> T { fn id<T>(x: T) -> T { x } id(value) }",
        "fn outer() -> fn(i32) -> i32 { fn id<T>(x: T) -> T { x } id }",
        "fn outer() -> fn(i32) -> i32 { fn inc(x: i32) -> i32 { x + 1 } inc }",
        "fn outer() -> i32 { fn f() -> i32 { 1 } val f = || 2; { fn f() -> i32 { 3 } f(); } f() }",
        "fn outer() -> fn() -> i32 { fn make(x: i32) -> fn() -> i32 { || x } make(42) }",
        "fn outer() -> i32 { val closure = || { fn inner() -> i32 { 42 } inner() }; closure() }",
        "struct S {} impl S { fn method(self) -> i32 { fn inner() -> i32 { 42 } inner() } }",
        "async fn outer() -> i32 { async fn inner() -> i32 { 42 } inner().await }",
        "fn outer() -> i32 { val answer = inner(); fn inner() -> i32 { 42 } answer }",
    ] {
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
}

#[test]
fn local_function_boundaries_reject_capture_and_leakage() {
    for (text, expected) in [
        (
            "fn outer(x: i32) { fn inner() -> i32 { x } }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "const x: i32 = 99; fn outer(x: i32) { fn inner() -> i32 { x } }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "fn outer() { var x = 0; fn inner() { x = 1; } }",
            "KG_TYPE_INVALID_ASSIGNMENT_TARGET",
        ),
        (
            "fn outer(x: i32) { fn inner() -> fn() -> i32 { || x } }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "fn outer<T>(x: T) { fn inner(x: T) {} }",
            "KG_TYPE_UNKNOWN_TYPE",
        ),
        (
            "struct S {} impl S { fn outer(self) { fn inner() { self; } } }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "fn outer() { fn inner() {} fn inner() {} }",
            "KG_RESOLVE_DUPLICATE_DECLARATION",
        ),
        (
            "fn outer() { { fn inner() {} } inner(); }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "fn outer() { fn inner() {} } fn other() { inner(); }",
            "KG_RESOLVE_UNKNOWN_NAME",
        ),
        (
            "fn outer() -> fn(f32) -> f32 { fn id<T: std::hash::Hash>(x: T) -> T { x } id }",
            "KG_TYPE_GENERIC_BOUND_NOT_SATISFIED",
        ),
        (
            "fn outer() { #[reflect] fn inner() {} }",
            "KG_ATTRIBUTE_UNSUPPORTED",
        ),
    ] {
        let source = SourceFile::new("locals.kgr", text);
        let result = analyze_source(&source, catalog::shared().unwrap()).unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected),
            "{text}\n{:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err(), "{text}");
    }
    for text in [
        "fn outer() { pub fn inner() {} }",
        "const N: i32 = { fn inner() -> i32 { 42 } 42 };",
    ] {
        let result = analyze_source(
            &SourceFile::new("locals.kgr", text),
            catalog::shared().unwrap(),
        )
        .unwrap();
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::UnsupportedSyntax { .. })),
            "{:?}",
            result.diagnostics()
        );
        assert!(result.into_codegen().is_err());
    }
}
