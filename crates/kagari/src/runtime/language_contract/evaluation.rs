use super::{Case, Expected, Value, run_routes};

#[test]
fn resolved_runtime_helpers() {
    run_routes(&Case::new("resolved-runtime-helpers", "struct Cell { var n: i32 } fn main() -> i32 { val c = Cell { n: 1 }; val xs = [1]; set_field(c, \"n\", 2); set_index(xs, 0, 3); print(type_of(7)); get_field(c, \"n\") + xs[0] }", Expected::Value(|_| Value::I32(5))).effects(&["i32"], &["i32"]).reflection());
}

#[test]
fn shadowed_print_has_no_host_effect() {
    run_routes(&Case::new(
        "shadowed-print-has-no-host-effect",
        "fn print(n: i32) -> i32 { n + 1 } fn main() -> i32 { print(6) }",
        Expected::Value(|_| Value::I32(7)),
    ));
}

#[test]
fn bare_function_is_not_return_value() {
    run_routes(&Case::new(
        "bare-function-is-not-return-value",
        "fn answer() -> i32 { 42 } fn main() -> i32 { answer }",
        Expected::Diagnostic("KG_TYPE_INVALID_VALUE_TARGET"),
    ));
}

#[test]
fn bare_helper_is_not_a_value() {
    run_routes(&Case::new(
        "bare-helper-is-not-a-value",
        "fn main() { val f = print; }",
        Expected::Diagnostic("KG_TYPE_INVALID_VALUE_TARGET"),
    ));
}

#[test]
fn duplicate_constant() {
    run_routes(&Case::new(
        "duplicate-constant",
        "const n: i32 = 1; const n: i32 = 2; fn main() -> i32 { n }",
        Expected::Diagnostic("KG_RESOLVE_DUPLICATE_DECLARATION"),
    ));
}

#[test]
fn heap_overflow_preserves_earlier_write() {
    run_routes(
        &Case::new(
            "heap-overflow-preserves-earlier-write",
            "use observe as test; fn main() { val a = test::vector(); a[1] = 42; a[0] += 1; }",
            Expected::ScriptTrap("integer overflow"),
        )
        .vector(&[2147483647, 0], &[2147483647, 42]),
    );
}

#[test]
fn heap_removed_target_not_recreated() {
    run_routes(&Case::new("heap-removed-target-not-recreated", "use observe as test; fn main() { val a = test::vector(); a[0] += if true { a.clear(); print(\"removed\"); 2 } else { 0 }; }", Expected::IndexTrap).vector(&[1], &[]).effects(&["removed"], &["removed"]));
}

#[test]
fn heap_compound_reads_rhs_write() {
    run_routes(&Case::new("heap-compound-reads-rhs-write", "use observe as test; fn main() { val a = test::vector(); a[0] += if true { a[0] = 10; 2 } else { 0 }; }", Expected::Value(|_| Value::Unit)).vector(&[1], &[12]));
}

#[test]
fn host_reject() {
    let mut reject = Case::new(
        "host_reject",
        "fn main() { print(\"first\"); print(\"rejected\"); print(\"unreachable\"); }",
        Expected::HostFailure,
    )
    .effects(&["first", "rejected"], &["first"]);
    reject.reject_call = Some(2);
    run_routes(&reject);
}

#[test]
fn heap_host_rejection_preserves_earlier_write() {
    let mut heap_reject = Case::new(
        "heap-host-rejection-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::vector(); a[0] = 42; print(\"committed\"); a[0] += if true { print(\"rejected\"); 2 } else { 0 }; }",
        Expected::HostFailure,
    ).vector(&[1], &[42]).effects(&["committed", "rejected"], &["committed"]);
    heap_reject.reject_call = Some(3); // Vec provider, committed log, rejected log.
    run_routes(&heap_reject);
}

#[test]
fn heap_cancellation_preserves_earlier_write() {
    let mut heap_cancel = Case::new(
        "heap-cancellation-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::vector(); a[0] = 42; a[0] += if true { print(\"cancel\"); 2 } else { 0 }; print(\"unreachable\"); }",
        Expected::Cancelled,
    ).vector(&[1], &[42]).effects(&["cancel"], &["cancel"]);
    heap_cancel.cancel_call = Some(2);
    run_routes(&heap_cancel);
}

#[test]
fn heap_loop_cancellation_preserves_earlier_write() {
    let mut heap_loop_cancel = Case::new(
        "heap-loop-cancellation-preserves-earlier-write",
        "use observe as test; fn main() { val a = test::vector(); a[0] = 42; print(\"committed\"); a[0] += spin(); } fn spin() -> i32 { loop {} }",
        Expected::Cancelled,
    ).vector(&[1], &[42]).effects(&["committed"], &["committed"]);
    heap_loop_cancel.cancel_at = Some(100);
    run_routes(&heap_loop_cancel);
}

#[test]
fn iteration_invalidated_by_push() {
    let operation = "a.push(9)";
    let source = format!(
        "use observe as test; fn main() {{ val a = test::vector(); val cursor = a.iter(); a[0] = 42; print(\"before\"); {operation}; print(\"committed\"); cursor.next(); print(\"unreachable\"); }}"
    );
    let case = Case::new(
        "iteration-detects-committed-write",
        &source,
        Expected::ScriptTrap("invalid or structurally modified iterator"),
    )
    .vector(&[1, 2], &[42, 2, 9])
    .effects(&["before", "committed"], &["before", "committed"]);
    run_routes(&case);
}

#[test]
fn iteration_invalidated_by_insert() {
    let operation = "a.insert(a.len(), 9)";
    let source = format!(
        "use observe as test; fn main() {{ val a = test::vector(); val cursor = a.iter(); a[0] = 42; print(\"before\"); {operation}; print(\"committed\"); cursor.next(); print(\"unreachable\"); }}"
    );
    let case = Case::new(
        "iteration-detects-committed-write",
        &source,
        Expected::ScriptTrap("invalid or structurally modified iterator"),
    )
    .vector(&[1, 2], &[42, 2, 9])
    .effects(&["before", "committed"], &["before", "committed"]);
    run_routes(&case);
}

#[test]
fn iteration_invalidated_by_pop() {
    let operation = "a.pop()";
    let source = format!(
        "use observe as test; fn main() {{ val a = test::vector(); val cursor = a.iter(); a[0] = 42; print(\"before\"); {operation}; print(\"committed\"); cursor.next(); print(\"unreachable\"); }}"
    );
    let case = Case::new(
        "iteration-detects-committed-write",
        &source,
        Expected::ScriptTrap("invalid or structurally modified iterator"),
    )
    .vector(&[1, 2], &[42])
    .effects(&["before", "committed"], &["before", "committed"]);
    run_routes(&case);
}

#[test]
fn iteration_invalidated_by_remove() {
    let operation = "a.remove(a.len() - a.len())";
    let source = format!(
        "use observe as test; fn main() {{ val a = test::vector(); val cursor = a.iter(); a[0] = 42; print(\"before\"); {operation}; print(\"committed\"); cursor.next(); print(\"unreachable\"); }}"
    );
    let case = Case::new(
        "iteration-detects-committed-write",
        &source,
        Expected::ScriptTrap("invalid or structurally modified iterator"),
    )
    .vector(&[1, 2], &[2])
    .effects(&["before", "committed"], &["before", "committed"]);
    run_routes(&case);
}

#[test]
fn iteration_invalidated_by_clear() {
    let operation = "a.clear()";
    let source = format!(
        "use observe as test; fn main() {{ val a = test::vector(); val cursor = a.iter(); a[0] = 42; print(\"before\"); {operation}; print(\"committed\"); cursor.next(); print(\"unreachable\"); }}"
    );
    let case = Case::new(
        "iteration-detects-committed-write",
        &source,
        Expected::ScriptTrap("invalid or structurally modified iterator"),
    )
    .vector(&[1, 2], &[])
    .effects(&["before", "committed"], &["before", "committed"]);
    run_routes(&case);
}

#[test]
fn return_discards_following_effects() {
    run_routes(&Case::new(
        "return-discards-following-effects",
        "fn main() -> i32 { return 7; print(\"unreachable\"); 42 }",
        Expected::Value(|_| Value::I32(7)),
    ));
}

#[test]
fn duplicate_field_declarations() {
    run_routes(&Case::new(
        "duplicate-field-declarations",
        "struct P { val x: i32, var x: i32 } fn main() {}",
        Expected::Diagnostic("KG_RESOLVE_DUPLICATE_FIELD"),
    ));
}

#[test]
fn field_initializers_follow_source_order() {
    run_routes(&Case::new("field-initializers-follow-source-order", "struct P { var left: i32, var right: i32 } fn left() -> i32 { print(\"left\"); 1 } fn right() -> i32 { print(\"right\"); 2 } fn main() -> i32 { val p = P { right: right(), left: left() }; p.left += p.right; p.left * 10 + p.right }", Expected::Value(|_| Value::I32(32))).effects(&["right", "left"], &["right", "left"]));
}

#[test]
fn string_len_counts_utf8_bytes() {
    run_routes(&Case::new(
        "string-len-counts-utf8-bytes",
        "fn main() -> usize { \"aé文\".len() }",
        Expected::Value(|_| Value::U64(6)),
    ));
}

#[test]
fn iter_array_option() {
    run_routes(&Case::new(
        "iter-array-option",
        "fn main() -> (usize, i32, bool) { val a = [4, 7]; { val cursor = a.iter(); cursor.next(); val second = match cursor.next() { Some(x) => x, None => 0 }; (a.len(), second, match cursor.next() { Some(x) => false, None => true }) } }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::U64(2), Value::I32(7), Value::Bool(true)])
                .unwrap()
        }),
    ));
}

#[test]
fn pop_empty_option() {
    run_routes(&Case::new(
        "pop-empty-option",
        "fn main() -> (i32, bool, usize) { val a = Vec::from([7]); val alias = a; val popped = match a.pop() { Some(x) => x, None => 0 }; (popped, match alias.pop() { Some(x) => false, None => true }, a.len()) }",
        Expected::Value(|heap| {
            heap.alloc_tuple(vec![Value::I32(7), Value::Bool(true), Value::U64(0)])
                .unwrap()
        }),
    ));
}

#[test]
fn user_print_is_direct_call() {
    run_routes(&Case::new(
        "user-print-is-direct-call",
        "fn print(n: i32) -> i32 { n + 1 } fn main() -> i32 { print(41) }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn local_print_is_not_a_helper() {
    run_routes(&Case::new(
        "local-print-is-not-a-helper",
        "fn main() { val print = 1; print(2); }",
        Expected::Diagnostic("KG_TYPE_INVALID_CALL_TARGET"),
    ));
}

#[test]
fn compound_reads_current_local() {
    run_routes(&Case::new(
        "compound-reads-current-local",
        "fn main() -> i32 { var n = 1; n += if true { n = 10; 2 } else { 0 }; n }",
        Expected::Value(|_| Value::I32(12)),
    ));
}

#[test]
fn compound_captures_index() {
    run_routes(&Case::new(
        "compound-captures-index",
        "fn main() -> i32 { val a = [1, 2]; var i = 0; a[i] += if true { i = 1; 2 } else { 0 }; a[0] * 10 + a[1] }",
        Expected::Value(|_| Value::I32(32)),
    ));
}

#[test]
fn compound_keeps_root_identity() {
    run_routes(&Case::new(
        "compound-keeps-root-identity",
        "fn main() -> i32 { var a = [1]; val old = a; a[0] += if true { a = [100]; 2 } else { 0 }; old[0] * 1000 + a[0] }",
        Expected::Value(|_| Value::I32(3100)),
    ));
}

#[test]
fn compound_reads_current_tuple() {
    run_routes(&Case::new(
        "compound-reads-current-tuple",
        "fn main() -> i32 { var t = (1, 2); t[0] += if true { t = (10, 20); 2 } else { 0 }; t[0] + t[1] }",
        Expected::Value(|_| Value::I32(32)),
    ));
}

#[test]
fn local_compound_overflow() {
    run_routes(&Case::new(
        "local-compound-overflow",
        "fn main() -> i32 { var n = 2147483647; n += 1; n }",
        Expected::ScriptTrap("integer overflow"),
    ));
}

#[test]
fn compound_scalars() {
    run_routes(&Case::new(
        "compound-scalars",
        "fn main() -> i32 { var n = 10; n += 5; n -= 3; n *= 2; n /= 4; n }",
        Expected::Value(|_| Value::I32(6)),
    ));
}

#[test]
fn assignment_evaluates_target_first() {
    run_routes(
        &Case::new(
            "assignment-evaluates-target-first",
            r#"
            fn root(a: [i32]) -> [i32] { print("root"); a }
            fn index() -> i32 { print("index"); 0 }
            fn rhs(a: [i32]) -> i32 { print("rhs"); a[0] = 20; 2 }
            fn main() -> i32 { val a = [1]; root(a)[index()] += rhs(a); a[0] }
        "#,
            Expected::Value(|_| Value::I32(22)),
        )
        .effects(&["root", "index", "rhs"], &["root", "index", "rhs"]),
    );
}

#[test]
fn plain_assignment_evaluates_target_first() {
    run_routes(
        &Case::new(
            "plain-assignment-evaluates-target-first",
            r#"
            fn root(a: [i32]) -> [i32] { print("root"); a }
            fn index() -> i32 { print("index"); 0 }
            fn rhs() -> i32 { print("rhs"); 42 }
            fn main() -> i32 { val a = [1]; root(a)[index()] = rhs(); a[0] }
        "#,
            Expected::Value(|_| Value::I32(42)),
        )
        .effects(&["root", "index", "rhs"], &["root", "index", "rhs"]),
    );
}

#[test]
fn nested_location_evaluated_once() {
    run_routes(
        &Case::new(
            "nested-location-evaluated-once",
            r#"
            struct Point { var x: i32 }
            fn index() -> i32 { print("index"); 0 }
            fn rhs(a: [Point]) -> i32 { print("rhs"); a[0] = Point { x: 20 }; 2 }
            fn main() -> i32 { val a = [Point { x: 1 }]; a[index()].x += rhs(a); a[0].x }
        "#,
            Expected::Value(|_| Value::I32(22)),
        )
        .effects(&["index", "rhs"], &["index", "rhs"]),
    );
}

#[test]
fn rhs_removes_compound_target() {
    run_routes(
        &Case::new(
            "rhs-removes-compound-target",
            r#"
            fn rhs(a: Vec<i32>) -> i32 { a.pop(); print("removed"); 2 }
            fn main() -> i32 { val a = Vec::from([1]); a[0] += rhs(a); print("written"); 0 }
        "#,
            Expected::IndexTrap,
        )
        .effects(&["removed"], &["removed"]),
    );
}

#[test]
fn rhs_repairs_missing_target() {
    run_routes(&Case::new(
        "rhs-repairs-missing-target",
        "fn rhs(a: Vec<i32>) -> i32 { a.push(20); 2 } fn main() -> i32 { val a = Vec::from([1]); a[1] += rhs(a); a[1] }",
        Expected::Value(|_| Value::I32(22)),
    ));
}

#[test]
fn compound_overflow() {
    run_routes(&Case::new(
        "compound-overflow",
        "fn main() -> i32 { val a = [2147483647]; a[0] += 1; a[0] }",
        Expected::ScriptTrap("integer overflow"),
    ));
}

#[test]
fn tuple_copy_commit() {
    run_routes(&Case::new(
        "tuple-copy-commit",
        "fn main() -> i32 { var t = ((1, 2), 3); val old = t; t[0][1] += 40; t[0][1] + old[0][1] }",
        Expected::Value(|_| Value::I32(44)),
    ));
}

#[test]
fn tuple_in_array_commit() {
    run_routes(&Case::new(
        "tuple-in-array-commit",
        "fn main() -> i32 { val a = [(1, 2)]; a[0][1] += 40; a[0][1] }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn rebind_array_slot() {
    run_routes(&Case::new(
        "rebind-array-slot",
        "fn main() -> i32 { var a = [1]; val old = a; a = [42]; a[0] + old[0] }",
        Expected::Value(|_| Value::I32(43)),
    ));
}

#[test]
fn reject_compound_val() {
    run_routes(&Case::new(
        "reject-compound-val",
        "fn main() -> i32 { val n = 1; n += 1; n }",
        Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET"),
    ));
}

#[test]
fn reject_val_tuple_write() {
    run_routes(&Case::new(
        "reject-val-tuple-write",
        "fn main() -> i32 { val t = (1, 2); t[0] += 1; t[0] }",
        Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET"),
    ));
}

#[test]
fn reject_compound_bool() {
    run_routes(&Case::new(
        "reject-compound-bool",
        "fn main() -> bool { var n = true; n += false; n }",
        Expected::Diagnostic("KG_TYPE_BINARY_OPERAND_TYPE_MISMATCH"),
    ));
}

#[test]
fn native_scalar_arithmetic() {
    run_routes(
        &Case::new(
            "native-scalar-arithmetic",
            "fn main() -> i32 { -(6 * 7) }",
            Expected::Value(|_| Value::I32(-42)),
        )
        .native(),
    );
}

#[test]
fn native_scalar_comparison() {
    run_routes(
        &Case::new(
            "native-scalar-comparison",
            "fn main() -> bool { !!(40 + 2 == 42) }",
            Expected::Value(|_| Value::Bool(true)),
        )
        .native(),
    );
}

#[test]
fn min_literal() {
    run_routes(
        &Case::new(
            "min-literal",
            "fn main() -> i32 { -2147483648 }",
            Expected::Value(|_| Value::I32(i32::MIN)),
        )
        .native(),
    );
}

#[test]
fn const_min_literal() {
    run_routes(
        &Case::new(
            "const-min-literal",
            "const MIN: i32 = -2147483648; fn main() -> i32 { MIN }",
            Expected::Value(|_| Value::I32(i32::MIN)),
        )
        .native(),
    );
}

#[test]
fn negate_min_literal() {
    run_routes(
        &Case::new(
            "negate-min-literal",
            "fn main() -> i32 { -(-2147483648) }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn invalid_positive_literal() {
    run_routes(&Case::new(
        "invalid-positive-literal",
        "fn main() -> i32 { 2147483648 }",
        Expected::Diagnostic("KG_TYPE_INVALID_LITERAL"),
    ));
}

#[test]
fn invalid_negative_literal() {
    run_routes(&Case::new(
        "invalid-negative-literal",
        "fn main() -> i32 { -2147483649 }",
        Expected::Diagnostic("KG_TYPE_INVALID_LITERAL"),
    ));
}

#[test]
fn invalid_pattern_literal() {
    run_routes(&Case::new(
        "invalid-pattern-literal",
        "fn main() -> i32 { match 1 { 2147483648 => 10, _ => 20 } }",
        Expected::Diagnostic("KG_TYPE_INVALID_LITERAL"),
    ));
}

#[test]
fn mismatched_pattern() {
    run_routes(&Case::new(
        "mismatched-pattern",
        "fn main() -> i32 { match true { 1 => 10, _ => 20 } }",
        Expected::Diagnostic("KG_TYPE_PATTERN_MISMATCH"),
    ));
}

#[test]
fn checked_pattern_literal() {
    run_routes(&Case::new(
        "checked-pattern-literal",
        "fn main() -> i32 { match 2147483647 { 2147483647 => 42, _ => 0 } }",
        Expected::Value(|_| Value::I32(42)),
    ));
}

#[test]
fn const_short_circuit() {
    run_routes(&Case::new(
        "const-short-circuit",
        "const A: bool = false && (1 / 0 == 0); const B: bool = true || (2147483647 + 1 == 0); fn main() -> bool { !A && B }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn const_overflow() {
    run_routes(&Case::new(
        "const-overflow",
        "const BAD: i32 = 2147483647 + 1; fn main() -> i32 { BAD }",
        Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER"),
    ));
}

#[test]
fn const_divide_zero() {
    run_routes(&Case::new(
        "const-divide-zero",
        "const BAD: i32 = 1 / 0; fn main() -> i32 { BAD }",
        Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER"),
    ));
}

#[test]
fn const_short_circuit_still_requires_const_safe_code() {
    run_routes(&Case::new(
        "const-short-circuit-still-requires-const-safe-code",
        "const BAD: bool = true || effect(); fn effect() -> bool { print(\"no\"); true } fn main() -> bool { BAD }",
        Expected::Diagnostic("KG_TYPE_INVALID_CONST_INITIALIZER"),
    ));
}

#[test]
fn add_overflow() {
    run_routes(
        &Case::new(
            "add_overflow",
            "fn main() -> i32 { 2147483647 + 1 }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn temporary_overflow() {
    run_routes(
        &Case::new(
            "temporary_overflow",
            "fn main() -> i32 { (2147483647 + 1) - 1 }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn sub_overflow() {
    run_routes(
        &Case::new(
            "sub_overflow",
            "fn main() -> i32 { (-2147483647 - 1) - 1 }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn mul_overflow() {
    run_routes(
        &Case::new(
            "mul_overflow",
            "fn main() -> i32 { 50000 * 50000 }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn neg_overflow() {
    run_routes(
        &Case::new(
            "neg_overflow",
            "fn main() -> i32 { -(-2147483647 - 1) }",
            Expected::ScriptTrap("integer overflow"),
        )
        .native(),
    );
}

#[test]
fn div_overflow() {
    run_routes(&Case::new(
        "div_overflow",
        "fn main() -> i32 { (-2147483647 - 1) / -1 }",
        Expected::ScriptTrap("integer overflow"),
    ));
}

#[test]
fn division_by_zero() {
    run_routes(&Case::new(
        "division_by_zero",
        "fn main() -> i32 { 1 / 0 }",
        Expected::ScriptTrap("integer division by zero"),
    ));
}

#[test]
fn overflow_effects() {
    run_routes(&Case::new("overflow_effects", "fn left() -> i32 { print(\"left\"); 2147483647 } fn right() -> i32 { print(\"right\"); 1 } fn main() -> i32 { left() + right() }", Expected::ScriptTrap("integer overflow")).effects(&["left", "right"], &["left", "right"]));
}

#[test]
fn scalar() {
    run_routes(&Case::new(
        "scalar",
        "fn main() -> i32 { (2 + 3) * 4 }",
        Expected::Value(|_| Value::I32(20)),
    ));
}

#[test]
fn alias() {
    run_routes(&Case::new(
        "alias",
        "struct P { var n: i32 } fn main() -> i32 { val a = P { n: 1 }; val b = a; b.n = 7; a.n }",
        Expected::Value(|_| Value::I32(7)),
    ));
}

#[test]
fn object_identity() {
    run_routes(&Case::new(
        "object_identity",
        "struct P { var n: i32 } fn main() -> bool { val a = P { n: 1 }; val b = P { n: 1 }; a === b }",
        Expected::Value(|_| Value::Bool(false)),
    ));
}

#[test]
fn tuple_value() {
    run_routes(&Case::new(
        "tuple_value",
        "fn main() -> bool { (1, \"a\") == (1, \"a\") }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn short_circuit() {
    run_routes(&Case::new(
        "short_circuit",
        "fn fail() -> bool { print(\"unreachable\"); val a = [1]; a[2] == 0 } fn main() -> bool { false && fail() }",
        Expected::Value(|_| Value::Bool(false)),
    ));
}

#[test]
fn left_to_right() {
    run_routes(&Case::new("left_to_right", "fn left() -> i32 { print(\"left\"); 1 } fn right() -> i32 { print(\"right\"); 2 } fn main() -> i32 { left() + right() }", Expected::Value(|_| Value::I32(3))).effects(&["left", "right"], &["left", "right"]));
}

#[test]
fn completed_effect_survives_trap() {
    run_routes(
        &Case::new(
            "completed_effect_survives_trap",
            "fn main() { print(\"committed\"); val a = [1]; a[9] = 2; print(\"unreachable\"); }",
            Expected::IndexTrap,
        )
        .effects(&["committed"], &["committed"]),
    );
}

#[test]
fn const_rebind() {
    run_routes(&Case::new(
        "const_rebind",
        "const N: i32 = 1; fn main() { N = 2; }",
        Expected::Diagnostic("KG_TYPE_INVALID_ASSIGNMENT_TARGET"),
    ));
}

#[test]
fn missing_name() {
    run_routes(&Case::new(
        "missing_name",
        "fn main() { missing; }",
        Expected::Diagnostic("KG_RESOLVE_UNKNOWN_NAME"),
    ));
}

#[test]
fn map_alias_through_call() {
    run_routes(&Case::new(
        "map_alias_through_call",
        "use std::collections::{HashMap};\nfn change(value: HashMap<String, i32>) -> HashMap<String, i32> { value.insert(\"key\", 42); value } fn main() -> bool { val a: HashMap<String, i32> = HashMap::new(); val b = change(a); val fresh: HashMap<String, i32> = HashMap::new(); fresh.insert(\"key\", 42); a === b && a !== fresh && a.get(\"key\") == b.get(\"key\") && a.len() == [0].len() }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}

#[test]
fn set_alias_through_call() {
    run_routes(&Case::new(
        "set_alias_through_call",
        "use std::collections::{HashSet};\nfn change(value: HashSet<String>) -> HashSet<String> { value.insert(\"key\"); value } fn main() -> bool { val a: HashSet<String> = HashSet::new(); val b = change(a); val fresh: HashSet<String> = HashSet::new(); fresh.insert(\"key\"); a === b && a !== fresh && a.contains(\"key\") }",
        Expected::Value(|_| Value::Bool(true)),
    ));
}
