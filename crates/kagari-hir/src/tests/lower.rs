use crate::{
    hir::{ExprKind, FunctionKind, Item, PatternKind, PlaceKind, StmtKind, TypeKind, Visibility},
    tests::common,
};

#[test]
fn lowers_items_into_hir_module() {
    let lowered = common::lower_ok(
        r#"
struct Player { hp: i32 }
enum Color { Red, Blue }
fn main(value: i32) -> i32 { value }
"#,
    );

    assert_eq!(lowered.module.items.len(), 3);
    assert!(matches!(lowered.module.items[0], Item::Struct(_)));
    assert!(matches!(lowered.module.items[1], Item::Enum(_)));
    assert!(matches!(lowered.module.items[2], Item::Function(_)));

    assert_eq!(lowered.module.structs[0].name, "Player");
    assert_eq!(lowered.module.enums[0].name, "Color");
    assert_eq!(lowered.module.functions[0].name, "main");
}

#[test]
fn lowers_function_body_expressions_and_statements() {
    let lowered = common::lower_ok(
        "fn main(value: i32) -> i32 { let next: i32 = value + 1; match next { _ => next } }",
    );
    let function = &lowered.module.functions[0];

    assert_eq!(function.params.len(), 1);
    assert!(matches!(
        &lowered.module.type_ref(function.params[0].ty).kind,
        TypeKind::Named(name) if name == "i32"
    ));

    let block = lowered.module.block(function.body);
    let stmt = lowered.module.stmt(block.statements[0]);
    match &stmt.kind {
        StmtKind::Let {
            local: _,
            mutable,
            name,
            ty,
            initializer,
        } => {
            assert!(!mutable);
            assert_eq!(name, "next");
            assert!(matches!(
                ty.map(|ty| &lowered.module.type_ref(ty).kind),
                Some(TypeKind::Named(name)) if name == "i32"
            ));
            assert!(matches!(
                &lowered.module.expr(*initializer).kind,
                ExprKind::Binary { .. }
            ));
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }

    let tail_expr = block.tail_expr.expect("expected tail expr");
    match &lowered.module.expr(tail_expr).kind {
        ExprKind::Match { arms, .. } => {
            assert_eq!(arms.len(), 1);
            assert!(matches!(
                lowered.module.pattern(arms[0].pattern).kind,
                PatternKind::Wildcard
            ));
        }
        other => panic!("unexpected tail expr: {other:?}"),
    }

    let assign = common::lower_ok("fn main(value: i32) -> i32 { value = 1; value }");
    let assign_block = assign.module.block(assign.module.functions[0].body);
    match &assign.module.stmt(assign_block.statements[0]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                assign.module.place(*target).kind,
                PlaceKind::Name(ref name) if name == "value"
            ));
        }
        other => panic!("unexpected assign stmt kind: {other:?}"),
    }

    let nested = common::lower_ok(
        r#"
struct Point { x: i32 }
struct Holder { inner: Point }

fn main() {
    let mut holder = Holder { inner: Point { x: 1 } };
    holder.inner.x = 2;
}
"#,
    );
    let block = nested.module.block(nested.module.functions[0].body);
    match &nested.module.stmt(block.statements[1]).kind {
        StmtKind::Assign { target, .. } => {
            assert!(matches!(
                &nested.module.place(*target).kind,
                PlaceKind::Field { name, .. } if name == "x"
            ));
        }
        other => panic!("unexpected nested assign stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_mutable_let_binding() {
    let lowered = common::lower_ok("fn main() { let mut value = 1; value = 2; }");
    let function = &lowered.module.functions[0];
    let block = lowered.module.block(function.body);
    let stmt = lowered.module.stmt(block.statements[0]);

    match &stmt.kind {
        StmtKind::Let { mutable, name, .. } => {
            assert!(*mutable);
            assert_eq!(name, "value");
        }
        other => panic!("unexpected stmt kind: {other:?}"),
    }
}

#[test]
fn lowers_top_level_statements_into_module_init_function() {
    let lowered = common::lower_ok(
        r#"
let boot = 1;

fn main() -> i32 { 1 }
"#,
    );

    let module_init = lowered
        .module
        .module_init
        .expect("expected implicit module init function");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == module_init)
        .expect("expected module init function in function list");

    assert_eq!(function.name, "__module_init__");
    assert!(matches!(function.kind, FunctionKind::ModuleInit));

    let block = lowered.module.block(function.body);
    assert_eq!(block.statements.len(), 1);
    assert!(matches!(
        lowered.module.stmt(block.statements[0]).kind,
        StmtKind::Let { .. }
    ));
}

#[test]
fn lowers_top_level_tail_expression_into_module_init_result() {
    let lowered = common::lower_ok(
        r#"
let boot = 1;

boot + 1
"#,
    );

    let module_init = lowered
        .module
        .module_init
        .expect("expected implicit module init function");
    let function = lowered
        .module
        .functions
        .iter()
        .find(|function| function.id == module_init)
        .expect("expected module init function in function list");

    let block = lowered.module.block(function.body);
    let tail_expr = block.tail_expr.expect("expected module init tail expr");
    assert!(matches!(
        lowered.module.expr(tail_expr).kind,
        ExprKind::Binary { .. }
    ));
}

#[test]
fn lowers_const_and_static_items_and_exports() {
    let lowered = common::lower_ok(
        r#"
pub const VERSION: i32 = 1;
pub static mut COUNTER: i32 = 0;
"#,
    );

    assert_eq!(lowered.module.items.len(), 2);
    assert_eq!(lowered.module.exports.len(), 2);

    let const_item = &lowered.module.consts[0];
    assert_eq!(const_item.name, "VERSION");
    assert!(matches!(const_item.visibility, Visibility::Public));
    assert!(matches!(
        lowered.module.expr(const_item.initializer).kind,
        ExprKind::Literal(_)
    ));

    let static_item = &lowered.module.statics[0];
    assert_eq!(static_item.name, "COUNTER");
    assert!(matches!(static_item.visibility, Visibility::Public));
    assert!(static_item.mutable);
    assert!(matches!(
        lowered.module.expr(static_item.initializer).kind,
        ExprKind::Literal(_)
    ));
}
