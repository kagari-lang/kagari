use super::*;
use crate::typeck::scalar::ScalarValue;
use kagari_stdlib::catalog as foundation_catalog;
use kagari_types::scalar::IntegerType;

#[test]
fn unresolved_body_holes_preserve_neighbor_facts_without_leaking_variables() {
    let source = SourceFile::new(
        "holes.kgr",
        "use std::collections::{List};\nfn bad() { val partial: (i32, List<_>) = (42, Vec::from([])); partial } fn good() -> u8 { 42 }",
    );
    let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis");
    assert!(!result.diagnostics().is_empty());
    let facts = result.facts();
    let good = facts
        .lowered
        .module
        .functions
        .iter()
        .find(|f| f.name == "good")
        .unwrap();
    assert_eq!(
        facts.typed.type_table.expr_type(
            facts
                .lowered
                .module
                .block(good.body.unwrap())
                .tail_expr
                .unwrap()
        ),
        Some(TypeId::Builtin(BuiltinType::U8))
    );
    let mut retained_tuple = false;
    for (id, _) in facts.lowered.module.body.expressions() {
        if let Some(ty) = facts.typed.type_table.expr_type(id) {
            ty.substitute_once(|member| {
                assert!(!matches!(member, TypeId::Inference(_)), "{ty:?}");
                None
            });
            if let TypeId::Tuple(members) = ty {
                retained_tuple |= members.first() == Some(&TypeId::Builtin(BuiltinType::I32));
            }
        }
    }
    assert!(retained_tuple);
    assert!(result.into_codegen().is_err());
}

#[test]
fn const_arithmetic_failures_preserve_other_semantic_facts() {
    for (expression, reason) in [
        ("2147483647 + 1", "integer overflow"),
        ("(-2147483647 - 1) - 1", "integer overflow"),
        ("50000 * 50000", "integer overflow"),
        ("-(-2147483647 - 1)", "integer overflow"),
        ("(-2147483647 - 1) / -1", "integer overflow"),
        ("1 / 0", "integer division by zero"),
        ("(2147483647 + 1) - 1", "integer overflow"),
    ] {
        let source = SourceFile::new(
            "const.kgr",
            format!(
                "const BAD: i32 = {expression}; const GOOD: i32 = 6 * 7; fn good() -> i32 {{ GOOD }}"
            ),
        );
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
            .expect("installed declaration analysis");
        let diagnostic = result.diagnostics().iter().find(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason: actual }
                if const_name == "BAD" && actual == reason
        )).unwrap_or_else(|| panic!("{expression}: {:?}", result.diagnostics()));
        let span = diagnostic.span.unwrap();
        assert!(span.start >= "const BAD: i32 = ".len());
        assert!(span.end <= "const BAD: i32 = ".len() + expression.len());
        let facts = result.facts();
        assert_eq!(facts.typed.const_values.len(), 1);
        assert_eq!(
            facts.typed.const_values.values().next(),
            Some(&ScalarValue::integer(42, IntegerType::I32).unwrap())
        );
        assert_eq!(
            facts.typed.functions[0].return_type,
            TypeId::Builtin(BuiltinType::I32)
        );
        assert!(result.into_codegen().is_err());
    }
}

#[test]
fn invalid_literals_and_patterns_retain_neighbor_types() {
    for source in [
        "fn bad() -> i32 { 2147483648 }",
        "fn bad() -> i32 { -2147483649 }",
        "fn bad() -> i32 { 99999999999999999999999999999999999999999 }",
        "fn bad() -> f32 { 99999999999999999999999999999999999999999.0 }",
        "fn bad() -> i32 { match 1 { 2147483648 => 1, _ => 2 } }",
        "const BAD: i32 = 2147483648;",
    ] {
        let source = SourceFile::new("literal.kgr", format!("{source} fn good() -> i32 {{ 42 }}"));
        let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
            .expect("installed declaration analysis");
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|diagnostic| matches!(diagnostic.kind, DiagnosticKind::InvalidLiteral { .. })),
            "{:?}",
            result.diagnostics()
        );
        assert_eq!(
            result
                .facts()
                .typed
                .functions
                .iter()
                .find(|function| function.name == "good")
                .unwrap()
                .return_type,
            TypeId::Builtin(BuiltinType::I32)
        );
        assert!(result.into_codegen().is_err());
    }
    let source = SourceFile::new(
        "literal.kgr",
        "fn main() -> i32 { match true { 1 => 1, _ => 2 } }",
    );
    let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis");
    assert!(
        result.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::PatternTypeMismatch { .. }
        ))
    );
    assert!(result.into_codegen().is_err());
}

#[test]
fn const_type_mismatch_is_rejected_before_codegen() {
    let source = SourceFile::new(
        "const.kgr",
        "const BAD: i32 = true; fn main() -> i32 { BAD }",
    );
    let result = crate::analyze_source(&source, foundation_catalog::shared().unwrap())
        .expect("installed declaration analysis");
    assert!(result.diagnostics().iter().any(|diagnostic| matches!(
        &diagnostic.kind, DiagnosticKind::InvalidConstInitializer { reason, .. }
            if reason == "expected `i32`, found `bool`"
    )));
    assert!(result.into_codegen().is_err());
}

#[test]
fn wide_const_dependencies_keep_values_and_error_owners_by_declaration_slot() {
    let mut text = String::new();
    for index in 0..2_000 {
        text.push_str(&format!("const C{index}: i32 = BASE + {index}; "));
    }
    text.push_str("const BASE: i32 = 42; const BAD: i32 = BASE / 0; fn good() -> i32 { C1999 }");
    let result = crate::analyze_source(
        &SourceFile::new("wide-const.kgr", text.clone()),
        foundation_catalog::shared().unwrap(),
    )
    .expect("installed declaration analysis");
    let facts = result.facts();
    assert_eq!(facts.typed.const_values.len(), 2_001);
    for (index, item) in facts.lowered.module.consts.iter().take(2_000).enumerate() {
        assert_eq!(
            facts.typed.const_values.get(&item.id),
            Some(&ScalarValue::integer(42 + index as i128, IntegerType::I32).unwrap())
        );
    }
    assert_eq!(result.diagnostics().len(), 1);
    let diagnostic = &result.diagnostics()[0];
    assert!(
        matches!(&diagnostic.kind, DiagnosticKind::InvalidConstInitializer { const_name, reason } if const_name == "BAD" && reason == "integer division by zero")
    );
    let span = diagnostic.span.unwrap();
    assert_eq!(&text[span.start..span.end], "BASE / 0");
    assert!(result.into_codegen().is_err());
}

#[test]
fn forward_const_annotations_preserve_cycle_and_initializer_errors() {
    for (text, expected) in [
        ("const A: i32 = B; const B: i32 = A;", "KG_TYPE_CONST_CYCLE"),
        (
            "const A: i32 = B; const B: i32 = true;",
            "KG_TYPE_INVALID_CONST_INITIALIZER",
        ),
    ] {
        let result = crate::analyze_source(
            &SourceFile::new("const-errors.kgr", text),
            foundation_catalog::shared().unwrap(),
        )
        .expect("installed declaration analysis");
        assert!(
            result
                .diagnostics()
                .iter()
                .any(|d| d.kind.code() == expected),
            "{:?}",
            result.diagnostics()
        );
        assert!(
            !result
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::InvalidValueTarget { .. }))
        );
        assert!(result.into_codegen().is_err());
    }
}
