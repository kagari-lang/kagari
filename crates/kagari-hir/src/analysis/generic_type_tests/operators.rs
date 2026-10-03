use super::contracts::analyze_contracts;
use super::*;

use {crate::hir::expr::ExprKind, kagari_source::diagnostic::DiagnosticKind};

#[test]
fn reflective_writes_share_target_context_and_recovery_member_comparison() {
    for (body, valid, mismatch) in [
        (
            "set_field(box, \"value\", Marker { value: 42 });",
            true,
            false,
        ),
        ("set_index(array, 0, Marker { value: 42 });", true, false),
        (
            "set_field(box, \"value\", Marker<bool> { value: 42 });",
            false,
            true,
        ),
        (
            "set_index(array, 0, Marker<bool> { value: 42 });",
            false,
            true,
        ),
        ("set_field(box, \"pair\", (1, missing));", false, false),
        ("set_field(box, \"pair\", (true, missing));", false, true),
        ("set_index(pairs, 0, (1, missing));", false, false),
        ("set_index(pairs, 0, (true, missing));", false, true),
    ] {
        let source = SourceFile::new(
            "reflective-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} struct Box {{ var value: Marker<i32>, var pair: (i32, bool) }} fn main() {{ val box = Box {{ value: Marker {{ value: 0 }}, pair: (1, true) }}; val array: ArrayList<Marker<i32>> = [Marker {{ value: 0 }}]; val pairs = [(1, true)]; {body} }}"
            ),
        );
        let analysis = crate::analyze_source(&source);
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::AssignmentTypeMismatch { .. }
            )),
            mismatch,
            "{body}"
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}

#[test]
fn declared_arguments_suppress_dependent_errors_but_keep_known_member_conflicts() {
    for (body, mismatch) in [
        ("values.push((1, missing));", false),
        ("values.push((true, missing));", true),
        ("put(values, (1, missing));", false),
        ("put(values, (true, missing));", true),
        ("list_count(missing);", false),
        ("list_count((1, missing));", true),
        ("text_pair(missing, \"x\");", false),
        ("text_pair(\"x\", missing);", false),
        ("text_pair(\"x\", (1, missing));", true),
    ] {
        let source = SourceFile::new(
            "standard-recovery.kgr",
            format!("fn bad() {{ val values = [(1, true)]; {body} }} fn good() -> i32 {{ 42 }}"),
        );
        let analysis = analyze_contracts(&source);
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(mismatch),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::ArgumentTypeMismatch { .. })),
            mismatch,
            "{body}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn declared_container_operands_supply_constructor_context_in_both_call_forms() {
    for (body, valid) in [
        ("values.push(Marker { value: 7 });", true),
        ("get_or(map.get(1), Marker { value: 7 });", true),
        ("get_or(map.get(1), Marker { value: 7 });", true),
        ("put(values, Marker { value: 7 });", true),
        ("map.insert(1, Marker { value: 7 });", true),
        ("put_map(map, 1, Marker { value: 7 });", true),
        ("values.push(Marker<bool> { value: 7 });", false),
        ("put_map(map, 1, Marker<bool> { value: 7 });", false),
        ("values.push(Marker { value: true });", false),
    ] {
        let source = SourceFile::new(
            "standard-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} fn main() {{ val values: ArrayList<Marker<i32>> = []; val map: HashMap<i32, Marker<i32>> = HashMap::new(); {body} }}"
            ),
        );
        let analysis = analyze_contracts(&source);
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid, "{body}");
    }
}

#[test]
fn declared_set_and_result_context_preserves_concrete_receiver_arguments() {
    for body in [
        "same_set(keys, HashSet::new());",
        "val readonly: Set<i32> = keys; same_set(readonly, HashSet::new());",
        "result_or(result, Marker { value: 7 });",
        "result_or(result, Marker { value: 7 });",
    ] {
        let analysis = analyze_contracts(&SourceFile::new(
            "standard-fallback-context.kgr",
            format!(
                "struct Marker<T> {{ val value: i32 }} fn check(keys: HashSet<i32>, result: Result<Marker<i32>, String>) {{ {body} }}"
            ),
        ));
        assert!(
            analysis.diagnostics().is_empty(),
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.into_codegen().is_ok());
    }
}

#[test]
fn boolean_operator_recovery_keeps_result_types_and_known_operand_conflicts() {
    for (expression, mismatch) in [
        ("missing == 1", false),
        ("missing < 1", false),
        ("missing < true", false),
        ("true < missing", false),
        ("missing < [1]", true),
        ("(view, missing) == (view, true)", true),
        ("missing && true", false),
        ("missing || 1", true),
        ("(1, missing) == (1, true)", false),
        ("(1, missing) == (false, true)", true),
        ("(1, missing) < (1, true)", true),
    ] {
        let source = SourceFile::new(
            "operator-recovery.kgr",
            format!(
                "trait View {{}} fn bad(view: View) {{ val result = {expression}; result; }} fn good() -> i32 {{ 42 }}"
            ),
        );
        let analysis = crate::analyze_source(&source);
        assert_eq!(
            analysis.diagnostics().len(),
            1 + usize::from(mismatch),
            "{expression}: {:?}",
            analysis.diagnostics()
        );
        let facts = analysis.facts();
        let result = facts
            .lowered
            .module
            .body
            .expressions()
            .find_map(|(id, expr)| {
                matches!(&expr.kind, ExprKind::Name { name, .. } if name == "result")
                    .then(|| facts.typed.type_table.expr_type(id))
                    .flatten()
            });
        assert_eq!(
            result,
            Some(TypeId::Builtin(BuiltinType::Bool)),
            "{expression}"
        );
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn unary_negation_uses_declared_signed_bounds_and_known_recovery_shapes() {
    for (source, valid, unary_error) in [
        (
            "fn negate<T: SignedNumber>(value: T) -> T { -value }",
            true,
            false,
        ),
        (
            "fn negate<T>(value: T) -> T where T: SignedNumber { -value }",
            true,
            false,
        ),
        ("fn negate<T>(value: T) -> T { -value }", false, true),
        (
            "fn negate<T: OrderedNumber>(value: T) -> T { -value }",
            false,
            true,
        ),
        (
            "fn negate<T: SignedNumber>(value: T) -> T { -value } fn call(value: u32) -> u32 { negate(value) }",
            false,
            false,
        ),
        ("fn bad() { -missing; }", false, false),
        ("fn bad() { -(1, missing); }", false, true),
    ] {
        let analysis = crate::analyze_source(&SourceFile::new("unary-bounds.kgr", source));
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{source}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(
            analysis
                .diagnostics()
                .iter()
                .any(|d| matches!(d.kind, DiagnosticKind::UnaryOperandTypeMismatch { .. })),
            unary_error,
            "{source}"
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn declared_math_and_equality_check_each_known_operand_after_recovery() {
    for (body, extra) in [
        ("ordered_pair(missing, 7);", 0),
        ("ordered_pair(missing, true);", 1),
        // The ordinary declaration infers T = i32 from the known operand.
        // Its bound holds once; the conflicting max argument is still checked.
        ("ordered_three(missing, 7, true);", 1),
        ("check_equal((1, missing), (1, true), \"test\");", 0),
        ("check_equal((1, missing), (false, true), \"test\");", 1),
        ("check_equal(missing, view, \"test\");", 1),
    ] {
        let analysis = analyze_contracts(&SourceFile::new(
            "standard-operand-recovery.kgr",
            format!("trait View {{}} fn bad(view: View) {{ {body} }}"),
        ));
        assert_eq!(
            analysis.diagnostics().len(),
            1 + extra,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert!(analysis.diagnostics().iter().any(|diagnostic| matches!(
            &diagnostic.kind, DiagnosticKind::UnknownName { name } if name == "missing"
        )));
        if body.starts_with("ordered_three") {
            let diagnostic = analysis.diagnostics().iter().find(|diagnostic| matches!(
                &diagnostic.kind,
                DiagnosticKind::ArgumentTypeMismatch { function_name, parameter_name, expected, found }
                    if function_name == "ordered_three" && parameter_name == "max" && expected == "i32" && found == "bool"
            )).expect("the remaining known operand must be checked after recovery");
            let span = diagnostic.span.unwrap();
            assert_eq!(
                &analysis.facts().lowered.source.text()[span.start..span.end],
                "true"
            );
        }
        assert!(analysis.into_codegen().is_err());
    }
}

#[test]
fn binary_rhs_uses_left_type_without_overriding_explicit_constructor_arguments() {
    for (body, valid) in [
        ("Token<i32>::Empty == Token::Empty", true),
        ("Token<i32>::Empty != Token::Empty", true),
        ("(Token<i32>::Empty, true) == (Token::Empty, true)", true),
        ("Token<i32>::Empty == Token<bool>::Empty", false),
        ("(Token<i32>::Empty, true) == (Token::Empty, 7)", false),
    ] {
        let source = SourceFile::new(
            "binary-context.kgr",
            format!("enum Token<T> {{ Empty }} fn main() -> bool {{ {body} }}"),
        );
        let analysis = crate::analyze_source(&source);
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}

#[test]
fn preceding_array_elements_and_completing_branches_supply_constructor_context() {
    for (body, valid) in [
        ("val values = [Token<i32>::Empty, Token::Empty];", true),
        (
            "val value = if true { Token<i32>::Empty } else { Token::Empty };",
            true,
        ),
        (
            "val value = match true { true => Token<i32>::Empty, false => Token::Empty };",
            true,
        ),
        (
            "val values = [Token<i32>::Empty, Token<bool>::Empty];",
            false,
        ),
        (
            "val value = if true { Token<i32>::Empty } else { Token<bool>::Empty };",
            false,
        ),
        (
            "val value = match true { true => Token<i32>::Empty, false => Token<bool>::Empty };",
            false,
        ),
        (
            "val value = if true { return; } else { Token::Empty };",
            false,
        ),
        (
            "val value = match true { true => { return; }, false => Token::Empty };",
            false,
        ),
    ] {
        let source = SourceFile::new(
            "sequence-context.kgr",
            format!("enum Token<T> {{ Empty }} fn main() {{ {body} }}"),
        );
        let analysis = crate::analyze_source(&source);
        assert_eq!(
            analysis.diagnostics().is_empty(),
            valid,
            "{body}: {:?}",
            analysis.diagnostics()
        );
        assert_eq!(analysis.into_codegen().is_ok(), valid);
    }
}
