use kagari_common::{DiagnosticKind, TypePosition};

use crate::{
    builtin::surface::{self, IterableProtocol},
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
struct Point { var x: i32 }
struct Holder { var inner: Point }
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
struct Point { var x: i32, var y: i32 }
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
struct Point { var x: i32 }

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
        common::lower_ok("fn main(value: i32) -> i32 { val next: i32 = value + 1; next }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let init_expr = match &let_stmt.kind {
        StmtKind::Binding { initializer, .. } => *initializer,
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
fn infers_array_method_call_types() {
    let lowered = common::lower_ok(
        r#"
fn main() -> usize {
    val values = [1, 2];
    val next = values.push(3);
    next.pop().len()
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);

    let push_expr = match &lowered.module.stmt(block.statements[1]).kind {
        StmtKind::Binding { initializer, .. } => *initializer,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(push_expr),
        Some(TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32))))
    );
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn infers_string_method_call_types() {
    let lowered = common::lower_ok(
        r#"
fn main(value: String) -> usize {
    value.len()
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::USize))
    );
}

#[test]
fn exposes_standard_builtin_surface_metadata() {
    assert!(surface::builtin_type("String").is_some());
    assert!(surface::builtin_type("usize").is_some());
    assert!(surface::builtin_type("str").is_none());

    let option = surface::standard_enum("Option").expect("Option should be standard");
    assert_eq!(option.arity, 1);
    assert_eq!(option.variants[0].name, "Some");
    assert_eq!(option.variants[1].name, "None");

    let result = surface::standard_enum("Result").expect("Result should be standard");
    assert_eq!(result.arity, 2);
    assert_eq!(result.variants[0].name, "Ok");
    assert_eq!(result.variants[1].name, "Err");

    assert!(surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::U64
    )));
    assert!(!surface::supports_const_type(&TypeId::Builtin(
        BuiltinType::String
    )));
    assert!(matches!(
        surface::iterable_protocol(&TypeId::Array(Box::new(TypeId::Builtin(BuiltinType::I32)))),
        Some(IterableProtocol::Array {
            item: TypeId::Builtin(BuiltinType::I32)
        })
    ));
}

#[test]
fn resolves_standard_builtin_type_annotations() {
    let lowered = common::lower_ok(
        r#"
fn choose(value: Option<i32>) -> Option<i32> { value }
fn fallible(value: Result<i32, String>) -> Result<i32, String> { value }
fn sized(value: usize) -> usize { value }
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");

    assert_eq!(
        typed.functions[0].return_type,
        TypeId::StandardEnum {
            name: "Option".to_string(),
            args: vec![TypeId::Builtin(BuiltinType::I32)],
        }
    );
    assert_eq!(
        typed.functions[1].return_type,
        TypeId::StandardEnum {
            name: "Result".to_string(),
            args: vec![
                TypeId::Builtin(BuiltinType::I32),
                TypeId::Builtin(BuiltinType::String),
            ],
        }
    );
    assert_eq!(
        typed.functions[2].return_type,
        TypeId::Builtin(BuiltinType::USize)
    );
}

#[test]
fn checks_standard_numeric_surface() {
    let lowered = common::lower_ok(
        r#"
fn signed(value: i16) -> i16 { -value }
fn unsigned(lhs: u64, rhs: u64) -> u64 { lhs + rhs }
fn float(lhs: f64, rhs: f64) -> bool { lhs < rhs }
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    check_module(&lowered, &names).expect("standard numeric types should check");

    let lowered = common::lower_ok("fn bad(value: u32) -> u32 { -value }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics = check_module(&lowered, &names).expect_err("unsigned negation should reject");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnaryOperandTypeMismatch {
                operator: "-",
                expected: "numeric".to_string(),
                found: "u32".to_string(),
            }
    }));
}

#[test]
fn checks_print_builtin_signature() {
    let lowered = common::lower_ok(r#"fn main() { print("hello"); }"#);
    let names = resolve_names(&lowered).expect("resolver should succeed");
    check_module(&lowered, &names).expect("print should accept str");

    let lowered = common::lower_ok("fn main() { print(1); }");
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject print argument");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArgumentTypeMismatch {
                function_name: "print".to_string(),
                parameter_name: "message".to_string(),
                expected: "String".to_string(),
                found: "i32".to_string(),
            }
    }));
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
    let lowered = common::lower_ok("fn foo() -> i32 { var x: i32 = 1; x = true; x }");
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
fn reports_condition_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { if 1 { 1 } else { 2 } }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject condition type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ConditionTypeMismatch {
                context: "if",
                found: "i32".to_string(),
            }
    }));
}

#[test]
fn reports_binary_operand_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> i32 { 1 + true }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject operands");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::BinaryOperandTypeMismatch {
                operator: "+",
                expected: "matching numeric".to_string(),
                lhs: "i32".to_string(),
                rhs: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_array_element_type_mismatch() {
    let lowered = common::lower_ok("fn foo() -> [i32] { [1, true] }");
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject array elements");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::ArrayElementTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
}

#[test]
fn reports_invalid_struct_initializers() {
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32, var y: bool }

fn foo() -> Point {
    Point { x: true, z: 1, x: 2 }
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");

    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject struct init");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::AssignmentTypeMismatch {
                expected: "i32".to_string(),
                found: "bool".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "unknown field `z`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "duplicate field `x`".to_string(),
            }
    }));
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidStructInitializer {
                struct_name: "Point".to_string(),
                reason: "missing field `y`".to_string(),
            }
    }));
}

#[test]
fn allows_assignment_to_var_local_but_not_val_local_or_param() {
    let var_local = common::lower_ok("fn foo() -> i32 { var x: i32 = 1; x = 2; x }");
    let names = resolve_names(&var_local).expect("resolver should succeed");
    let typed = check_module(&var_local, &names).expect("type checker should succeed");
    let function = &var_local.module.functions[0];
    let block = var_local.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let val_local = common::lower_ok("fn foo() -> i32 { val x: i32 = 1; x = 2; x }");
    let names = resolve_names(&val_local).expect("resolver should succeed");
    let diagnostics = check_module(&val_local, &names).expect_err("val local should reject write");
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);

    let param_assignment = common::lower_ok("fn foo(value: i32) -> i32 { value = 1; value }");
    let names = resolve_names(&param_assignment).expect("resolver should succeed");
    let diagnostics =
        check_module(&param_assignment, &names).expect_err("parameter should reject write");
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
}

#[test]
fn allows_assignment_to_var_field_and_index_places() {
    let field_assignment = common::lower_ok(
        r#"
struct Point { var x: i32 }
struct Holder { val inner: Point }

fn main() -> i32 {
    val holder = Holder { inner: Point { x: 1 } };
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

    let param_field_assignment = common::lower_ok(
        r#"
struct Point { var x: i32 }

fn main(point: Point) -> i32 {
    point.x = 3;
    point.x
}
"#,
    );
    let names = resolve_names(&param_field_assignment).expect("resolver should succeed");
    let typed = check_module(&param_field_assignment, &names)
        .expect("var field assignment through parameter should type check");
    let function = &param_field_assignment.module.functions[0];
    let block = param_field_assignment.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );

    let index_assignment = common::lower_ok(
        r#"
fn main() -> i32 {
    val values = [1, 2];
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
fn rejects_assignment_to_val_field() {
    let field_assignment = common::lower_ok(
        r#"
struct Point { val x: i32 }

fn main() -> i32 {
    val point = Point { x: 1 };
    point.x = 3;
    point.x
}
"#,
    );
    let names = resolve_names(&field_assignment).expect("resolver should succeed");
    let diagnostics =
        check_module(&field_assignment, &names).expect_err("val field should reject write");

    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
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
fn records_const_reference_types() {
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;

fn main() -> i32 { VERSION }
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
    assert!(typed.statics.is_empty());
}

#[test]
fn rejects_assignment_to_const() {
    let const_storage = common::lower_ok(
        r#"
const VERSION: i32 = 1;
fn main() -> i32 { VERSION = 2; 0 }
"#,
    );
    let names = resolve_names(&const_storage).expect("resolver should succeed");
    let diagnostics =
        check_module(&const_storage, &names).expect_err("type checker should reject writes");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, DiagnosticKind::InvalidAssignmentTarget);
}

#[test]
fn validates_trait_impl_and_interface_method_calls() {
    let lowered = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> String {
        self.name
    }
}

fn show_interface(value: Display) -> String {
    value.show()
}

fn show_static<T>(value: T) -> String
where T: Display
{
    value.show()
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let typed = check_module(&lowered, &names).expect("type checker should succeed");

    let show_interface = typed
        .functions
        .iter()
        .find(|function| function.name == "show_interface")
        .expect("expected show_interface");
    assert_eq!(
        show_interface.params[0].ty,
        TypeId::Trait("Display".to_string())
    );
    assert_eq!(
        show_interface.return_type,
        TypeId::Builtin(BuiltinType::String)
    );
}

#[test]
fn reports_unknown_trait_bounds() {
    let lowered = common::lower_ok(
        r#"
fn show<T>(value: T) -> T
where T: Missing
{
    value
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject unknown bound");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::UnknownTrait {
                trait_name: "Missing".to_string(),
            }
    }));
}

#[test]
fn rejects_interface_use_of_generic_trait_methods() {
    let lowered = common::lower_ok(
        r#"
trait Mapper {
    fn map<T>(self, value: T) -> T;
}

fn use_mapper(value: Mapper) {
    value;
}
"#,
    );
    let names = resolve_names(&lowered).expect("resolver should succeed");
    let diagnostics =
        check_module(&lowered, &names).expect_err("type checker should reject interface type");

    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::InvalidInterfaceType {
                trait_name: "Mapper".to_string(),
                reason: "method `map` is not interface-compatible".to_string(),
            }
    }));
}

#[test]
fn rejects_invalid_trait_impls() {
    let missing_method = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {}
"#,
    );
    let names = resolve_names(&missing_method).expect("resolver should succeed");
    let diagnostics =
        check_module(&missing_method, &names).expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "missing impl method".to_string(),
            }
    }));

    let wrong_return = common::lower_ok(
        r#"
trait Display {
    fn show(self) -> String;
}

struct Player {
    val name: String,
}

impl Display for Player {
    fn show(self) -> i32 {
        1
    }
}
"#,
    );
    let names = resolve_names(&wrong_return).expect("resolver should succeed");
    let diagnostics =
        check_module(&wrong_return, &names).expect_err("type checker should reject impl");
    assert!(diagnostics.iter().any(|diagnostic| {
        diagnostic.kind
            == DiagnosticKind::TraitMethodMismatch {
                trait_name: "Display".to_string(),
                method_name: "show".to_string(),
                reason: "return type expected `String`, found `i32`".to_string(),
            }
    }));
}
