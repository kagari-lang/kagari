use kagari_common::DiagnosticKind;

use crate::{
    hir::{ExprKind, PatternKind, StmtKind},
    resolver::{ResolvedName, resolve_names},
    tests::common,
};

#[test]
fn reports_duplicate_function_names() {
    let lowered = common::lower_ok(
        r#"
fn foo() {}
fn foo() {}
"#,
    );

    let diagnostics = resolve_names(&lowered).expect_err("resolver should reject duplicates");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].kind,
        DiagnosticKind::DuplicateFunction {
            name: "foo".to_string(),
        }
    );
    assert_eq!(
        diagnostics[0].to_string(),
        "Error: duplicate function `foo` at 13..24"
    );
}

#[test]
fn resolves_params_and_locals_in_function_body() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { val next: i32 = value; next }");
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let function = &lowered.module.functions[0];

    let block = lowered.module.block(function.body);
    let let_stmt = lowered.module.stmt(block.statements[0]);
    let (let_local, init_expr) = match &let_stmt.kind {
        StmtKind::Binding {
            local, initializer, ..
        } => (*local, *initializer),
        other => panic!("unexpected stmt kind: {other:?}"),
    };

    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        resolved.expr_resolution(init_expr),
        Some(ResolvedName::Param(function.params[0].id))
    );
    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Local(let_local))
    );
}

#[test]
fn resolves_named_match_pattern_bindings_inside_arm() {
    let lowered = common::lower_ok("fn main(value: i32) -> i32 { match value { bound => bound } }");
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    let (pattern_local, arm_expr) = match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => {
            let arm = &arms[0];
            let pattern_local = match &lowered.module.pattern(arm.pattern).kind {
                PatternKind::Name { local, .. } => *local,
                other => panic!("unexpected pattern kind: {other:?}"),
            };
            (pattern_local, arm.expr)
        }
        other => panic!("unexpected expr kind: {other:?}"),
    };

    assert_eq!(
        resolved.expr_resolution(arm_expr),
        Some(ResolvedName::Local(pattern_local))
    );
}

#[test]
fn resolves_const_names_in_function_body() {
    let lowered = common::lower_ok(
        r#"
const VERSION: i32 = 1;

fn main() -> i32 { VERSION }
"#,
    );
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("tail expr");

    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Const(lowered.module.consts[0].id))
    );
}

#[test]
fn collects_module_trait_impl_and_type_namespaces() {
    let lowered = common::lower_ok(
        r#"
mod gameplay;

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

fn main() -> i32 { 1 }
"#,
    );
    let resolved = resolve_names(&lowered).expect("resolver should succeed");

    assert!(resolved.items.contains_module("gameplay"));
    assert!(resolved.items.contains_trait("Display"));
    assert!(resolved.items.contains_struct("Player"));
    assert!(resolved.items.contains_function("main"));
    assert_eq!(resolved.items.impl_count(), 1);
}

#[test]
fn keeps_top_level_initialization_bindings_out_of_item_namespace() {
    let lowered = common::lower_ok(
        r#"
val boot = 1;
boot
"#,
    );
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let module_init = lowered
        .module
        .module_init
        .expect("expected module init function");
    let init_function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == module_init)
        .expect("expected module init function");
    let block = lowered.module.block(init_function.body);
    let binding_stmt = lowered.module.stmt(block.statements[0]);
    let binding_local = match &binding_stmt.kind {
        StmtKind::Binding { local, .. } => *local,
        other => panic!("unexpected stmt kind: {other:?}"),
    };
    let tail_expr = block.tail_expr.expect("expected module init tail expr");

    assert_eq!(
        resolved.expr_resolution(tail_expr),
        Some(ResolvedName::Local(binding_local))
    );
    assert!(!resolved.items.contains_const("boot"));
    assert!(!resolved.items.contains_function("boot"));

    let lowered = common::lower_ok(
        r#"
val boot = 1;
fn main() -> i32 { boot }
"#,
    );
    let resolved = resolve_names(&lowered).expect("resolver should succeed");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.name == "main")
        .expect("expected main function");
    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("expected tail expr");

    assert_eq!(resolved.expr_resolution(tail_expr), None);
}
