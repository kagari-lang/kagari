use kagari_common::{DiagnosticKind, TypePosition};

use crate::{
    hir::{ExprKind, PatternKind, StmtKind},
    resolver::resolve_names,
    tests::common,
    typeck::check_module,
    types::{BuiltinType, TypeId},
};

#[test]
fn reports_unknown_parameter_type() {
    let lowered = common::lower_ok("fn foo(value: number) {}");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Parameter,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown parameter type `number` in function `foo` at 13..20"
    );
}

#[test]
fn reports_unknown_return_type() {
    let lowered = common::lower_ok("fn foo() -> number {}");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject type");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::UnknownType {
            type_name: "number".to_string(),
            function_name: "foo".to_string(),
            position: TypePosition::Return,
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: unknown return type `number` in function `foo` at 11..18"
    );
}

#[test]
fn reports_invalid_const_initializer_expression() {
    let lowered = common::lower_ok("const VALUE: i32 = type_of(1);");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject const initializer");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::InvalidConstInitializer {
            const_name: "VALUE".to_string(),
            reason: "unsupported const initializer expression".to_string(),
        }
    );
}

#[test]
fn reports_reflection_write_on_const_value() {
    let lowered = common::lower_ok(
        r#"
struct Point { x: i32 }
struct Holder { inner: Point }
const ROOT: Holder = Holder { inner: Point { x: 1 } };

fn main() -> Point { set_field(ROOT.inner, "x", 2) }
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names)
        .expect_err("type checker should reject reflection write on const");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConstWriteNotAllowed {
                const_name: "ROOT".to_string(),
            }
    }));
}

#[test]
fn reports_const_dependency_cycle() {
    let lowered = common::lower_ok(
        r#"
const A: i32 = B;
const B: i32 = A;
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject cycle");

    assert!(diagnostics.iter().any(|diagnostic| diagnostic.kind
        == DiagnosticKind::ConstCycle {
            const_name: "A".to_string(),
        }));
}

#[test]
fn rejects_heap_backed_const_types() {
    let lowered = common::lower_ok(
        r#"
struct Point { x: i32, y: i32 }
const PAIR: (i32, i32) = (1, 2);
const VALUES: [i32] = [3, 4];
const POINT: Point = Point { x: 5, y: 6 };
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names)
        .expect_err("type checker should reject heap-backed const types");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "PAIR".to_string(),
                reason: "const type `(i32, i32)` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "VALUES".to_string(),
                reason: "const type `[i32]` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidConstInitializer {
                const_name: "POINT".to_string(),
                reason: "const type `Point` is heap-backed; const supports value types only"
                    .to_string(),
            }
    }));
}

#[test]
fn plain_function_calls_keep_fresh_return_flow() {
    let lowered = common::lower_ok(
        r#"
struct Point { x: i32 }

fn id(point: Point) -> Point { point }

fn main(point: Point) -> Point {
    id(point)
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should accept plain call");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Struct("Point".to_string()))
    );
}

#[test]
fn reports_function_call_argument_type_mismatch() {
    let lowered = common::lower_ok(
        r#"
fn add_one(value: i32) -> i32 { value + 1 }

fn main() -> i32 {
    add_one(true)
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject argument type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "add_one".to_string(),
                parameter_name: "value".to_string(),
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_function_call_arity_mismatch() {
    let lowered = common::lower_ok(
        r#"
fn answer() -> i32 { 42 }

fn main() -> i32 {
    answer(1)
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject arity mismatch");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::CallArityMismatch {
                function_name: "answer".to_string(),
                expected: 0,
                found: 1,
            }
    }));
}

#[test]
fn records_expression_types_for_resolved_body_expressions() {
    let lowered =
        common::lower_ok("fn main(value: i32) -> i32 { let next: i32 = value + 1; next }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let init_expr = match &let_stmt.kind {
        StmtKind::Let { initializer, .. } => *initializer,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(init_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn reports_return_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { true }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject return");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::ReturnTypeMismatch {
            function_name: "foo".to_string(),
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_break_and_continue_outside_loop() {
    let lowered = common::lower_ok("fn foo() { break; continue; }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject control flow");

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::BreakOutsideLoop);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::ContinueOutsideLoop);
}

#[test]
fn reports_invalid_assignment_target() {
    let lowered = common::lower_ok("fn foo() -> i32 { foo = 1; 0 }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject assignment target");

    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
}

#[test]
fn reports_assignment_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { let mut x: i32 = 1; x = true; x }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject assignment");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::AssignmentTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn allows_assignment_to_mutable_local_but_not_immutable_local_or_param() {
    let mutable_local = common::lower_ok("fn foo() -> i32 { let mut x: i32 = 1; x = 2; x }");
    let names = resolve_names(&mutable_local).expect("resolver should succeed");
    let typed = check_module(&mutable_local, &names).expect("type checker should succeed");
    let function = &mutable_local.module.functions[0];
    let block = mutable_local.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let immutable_local = common::lower_ok("fn foo() -> i32 { let x: i32 = 1; x = 2; x }");
    let names = resolve_names(&immutable_local).expect("resolver should succeed");
    let diagnostics =
        check_module(&immutable_local, &names).expect_err("immutable local should reject write");
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);

    let param_assignment = common::lower_ok("fn foo(value: i32) -> i32 { value = 1; value }");
    let names = resolve_names(&param_assignment).expect("resolver should succeed");
    let diagnostics =
        check_module(&param_assignment, &names).expect_err("parameter should reject write");
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
}

#[test]
fn allows_assignment_to_mutable_field_and_index_places() {
    let field_assignment = common::lower_ok(
        r#"
struct Point { x: i32 }
struct Holder { inner: Point }

fn main() -> i32 {
    let mut holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 3;
    holder.inner.x
}
"#,
    );
    let names = resolve_names(&field_assignment).expect("resolver should succeed");
    let typed =
        check_module(&field_assignment, &names).expect("field assignment should type check");
    let function = &field_assignment.module.functions[0];
    let block = field_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let index_assignment = common::lower_ok(
        r#"
fn main() -> i32 {
    let mut values = [1, 2];
    values[1] = 9;
    values[1]
}
"#,
    );
    let names = resolve_names(&index_assignment).expect("resolver should succeed");
    let typed =
        check_module(&index_assignment, &names).expect("index assignment should type check");
    let function = &index_assignment.module.functions[0];
    let block = index_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn reports_if_branch_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { if true { 1 } else { false } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject if");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::IfBranchTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn reports_match_arm_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { match 1 { 1 => 1, _ => false } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics = check_module(&lowered, &names).expect_err("type checker should reject match");

    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::MatchArmTypeMismatch {
            expected: "i32".to_string(),
            found: "bool".to_string(),
        }
    );
}

#[test]
fn records_named_match_pattern_binding_type() {
    let lowered = common::lower_ok("fn foo(value: i32) -> i32 { match value { bound => bound } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    let pattern_local = match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => match lowered.module.pattern(arms[0].pattern).kind {
            PatternKind::Name { local, .. } => local,
            ref other => panic!("unexpected pattern kind: {other:?}"),
        },
        ref other => panic!("unexpected expr kind: {other:?}"),
    };

    assert_eq!(
        typed.type_table.local_type(pattern_local),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn records_const_and_static_reference_types() {
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;
static mut COUNTER: i32 = 0;

fn main() -> i32 { VERSION + COUNTER }
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.consts.get(&lowered.module.consts[0].id),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed
            .statics
            .get(&lowered.module.statics[0].id)
            .map(|item| &item.ty),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn allows_assignment_to_static_mut_but_not_const_or_static() {
    let mutable_static = common::lower_ok(
        r#"
static mut COUNTER: i32 = 0;
fn main() -> i32 { COUNTER = 1; COUNTER }
"#,
    );
    let names = resolve_names(&mutable_static).expect("resolver should succeed");
    let typed = check_module(&mutable_static, &names).expect("type checker should succeed");
    let function = mutable_static
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = mutable_static.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let immutable_storage = common::lower_ok(
        r#"
const VERSION: i32 = 1;
static CACHE: i32 = 0;
fn main() -> i32 { VERSION = 2; CACHE = 3; 0 }
"#,
    );
    let names = resolve_names(&immutable_storage).expect("resolver should succeed");
    let diagnostics =
        check_module(&immutable_storage, &names).expect_err("type checker should reject writes");

    assert_eq!(diagnostics.len(), 2);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
    assert_eq!(diagnostics[1].kind, DiagnosticKind::InvalidAssignmentTarget);
}
