use super::*;
use kagari_common::identity::map::DefinitionContext;

#[test]
fn plain_function_calls_keep_fresh_return_flow() {
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok(
        r#"
struct Point { var x: i32 }

fn id(point: Point) -> Point { point }

fn main(point: Point) -> Point {
    id(point)
}
"#,
    );
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &lowered, &names, None)
        .into_checked()
        .expect("type checker should accept plain call");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body.unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Struct(crate::types::NominalType {
            associated_types: Default::default(),
            declaration: common::definition(
                &context.snapshot(),
                &lowered,
                kagari_common::identity::DefinitionKind::Struct,
                "Point"
            ),
            arguments: Vec::new()
        }))
    );
}

#[test]
fn records_expression_types_for_resolved_body_expressions() {
    let context = DefinitionContext::new().unwrap();
    let lowered =
        common::lower_ok("fn main(value: i32) -> i32 { val next: i32 = value + 1; next }");
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body.unwrap());
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
fn allows_assignment_to_var_local_but_not_val_local_or_param() {
    let context = DefinitionContext::new().unwrap();
    let var_local = common::lower_ok("fn foo() -> i32 { var x: i32 = 1; x = 2; x }");
    let names = resolve_names(&var_local)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &var_local, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &var_local.module.functions[0];
    let block = var_local.module.block(function.body.unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn allows_assignment_to_var_field_and_index_places() {
    let context = DefinitionContext::new().unwrap();
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
    let names = resolve_names(&field_assignment)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &field_assignment, &names, None)
        .into_checked()
        .expect("field assignment should type check");
    let function = &field_assignment.module.functions[0];
    let block = field_assignment.module.block(function.body.unwrap());
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
    let names = resolve_names(&param_field_assignment)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &param_field_assignment, &names, None)
        .into_checked()
        .expect("var field assignment through parameter should type check");
    let function = &param_field_assignment.module.functions[0];
    let block = param_field_assignment.module.block(function.body.unwrap());
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
    let names = resolve_names(&index_assignment)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &index_assignment, &names, None)
        .into_checked()
        .expect("index assignment should type check");
    let function = &index_assignment.module.functions[0];
    let block = index_assignment.module.block(function.body.unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");
    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
}

#[test]
fn records_named_match_pattern_binding_type() {
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok("fn foo(value: i32) -> i32 { match value { bound => bound } }");
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body.unwrap());
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
    let context = DefinitionContext::new().unwrap();
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;

fn main() -> i32 { VERSION }
"#,
    );
    let names = resolve_names(&lowered)
        .unwrap()
        .into_checked()
        .expect("resolver should succeed");
    let typed = check_module(&context, &lowered, &names, None)
        .into_checked()
        .expect("type checker should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body.unwrap());
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        typed.type_table.expr_type(tail_expr),
        Some(TypeId::Builtin(BuiltinType::I32))
    );
    assert_eq!(
        typed.consts.get(&lowered.module.consts[0].id),
        Some(&TypeId::Builtin(BuiltinType::I32))
    );
}
