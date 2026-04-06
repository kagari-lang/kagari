use crate::{
    hir::{ExprKind, Item, PatternKind, PlaceKind, StmtKind, TypeKind},
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
            name,
            ty,
            initializer,
        } => {
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
}
