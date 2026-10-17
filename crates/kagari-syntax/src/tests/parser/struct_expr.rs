use kagari_source::diagnostic::DiagnosticKind;

use crate::{
    ast::{
        expr::{Expr, StructExpr, StructUpdateBase},
        item::Item,
        stmt::Stmt,
        traits::AstNode,
    },
    kind::SyntaxKind,
    tests::common,
};

#[test]
fn explicit_enum_path_preserves_arguments_and_variant_name() {
    let text = "fn main() { model::Token<Map<i32, [bool]>>::Empty }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let Expr::PathExpr(path) = common::first_function(&module)
        .body()
        .unwrap()
        .tail_expr()
        .unwrap()
    else {
        panic!("enum path");
    };
    assert_eq!(path.name_text().as_deref(), Some("model::Token::Empty"));
    assert_eq!(path.generic_args().unwrap().args().count(), 1);
}

#[test]
fn explicit_constructor_arguments_preserve_nested_types_and_comparisons() {
    let text = "fn main() { model::Marker<Map<i32, [bool]>> { value: 7 } }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let Expr::StructExpr(expr) = common::first_function(&module)
        .body()
        .unwrap()
        .tail_expr()
        .unwrap()
    else {
        panic!("constructor");
    };
    assert_eq!(expr.generic_args().unwrap().args().count(), 1);
    let comparison = common::parse_ok("fn main() { a < b }");
    assert!(matches!(
        common::first_function(&comparison)
            .body()
            .unwrap()
            .tail_expr(),
        Some(Expr::BinaryExpr(_))
    ));
}

#[test]
fn parses_struct_literal_expression() {
    let module = common::parse_ok("fn main() { Player { hp: 10, name: value } }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::StructExpr(struct_expr) => {
            assert_eq!(
                struct_expr
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Player")
            );

            let field_list = struct_expr.field_list().expect("expected field list");
            let fields: Vec<_> = field_list.fields().collect();

            assert_eq!(fields.len(), 2);
            assert_eq!(fields[0].name_text().as_deref(), Some("hp"));
            match fields[0].value().expect("expected hp value") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("10")),
                other => panic!("unexpected hp value: {other:?}"),
            }

            assert_eq!(fields[1].name_text().as_deref(), Some("name"));
            match fields[1].value().expect("expected name value") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("value")),
                other => panic!("unexpected name value: {other:?}"),
            }
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}

#[test]
fn struct_update_base_keeps_explicit_fields_and_physical_children_separate() {
    for (constructor, names, operand_kind) in [
        ("Point { ..base }", vec![], SyntaxKind::PathExpr),
        (
            "Point { x: 1, y, ..make_point() }",
            vec!["x", "y"],
            SyntaxKind::CallExpr,
        ),
        (
            "model::Point<i32> { x, /* UTF-8: 😀 */ .. /* base */ make_point().next?\r\n }",
            vec!["x"],
            SyntaxKind::PropagateExpr,
        ),
        (
            "Point { ..Point { x: 1, ..base } }",
            vec![],
            SyntaxKind::StructExpr,
        ),
        (
            "Point { ..if ready { first } else { second } }",
            vec![],
            SyntaxKind::IfExpr,
        ),
        ("Point { ..|| base }", vec![], SyntaxKind::ClosureExpr),
        ("Point { ..{ base } }", vec![], SyntaxKind::BlockExpr),
    ] {
        let text = format!("fn main() {{ {constructor} }}");
        let module = common::parse_ok(&text);
        assert_eq!(module.syntax().to_string(), text);
        let Some(Expr::StructExpr(expr)) =
            common::first_function(&module).body().unwrap().tail_expr()
        else {
            panic!("expected struct expression: {constructor}");
        };
        let fields = expr.field_list().unwrap();
        assert_eq!(
            fields
                .fields()
                .map(|field| field.name_text().unwrap())
                .collect::<Vec<_>>(),
            names,
            "{constructor}"
        );
        let base = expr.update_base().expect("written base");
        assert_eq!(base.syntax().parent(), Some(expr.syntax().clone()));
        assert_eq!(base.value().unwrap().syntax().kind(), operand_kind);
        assert_eq!(
            base.syntax()
                .children_with_tokens()
                .filter(|child| child.kind() == SyntaxKind::DotDot)
                .count(),
            1
        );
        assert_eq!(expr.syntax().children().filter_map(Expr::cast).count(), 1);
        assert_eq!(base.syntax().children().filter_map(Expr::cast).count(), 1);
        assert!(!Expr::can_cast(SyntaxKind::StructUpdateBase));
        assert!(StructUpdateBase::can_cast(SyntaxKind::StructUpdateBase));
        assert!(
            fields
                .syntax()
                .children()
                .all(|child| child.kind() == SyntaxKind::FieldInit)
        );
    }
}

#[test]
fn struct_update_missing_operand_is_not_an_absent_marker() {
    for (constructor, has_marker, has_value) in [
        ("Point {}", false, false),
        ("Point { x, }", false, false),
        ("Point { ..base }", true, true),
        ("Point { .. }", true, false),
        ("Point { x, .. /* missing */ }", true, false),
    ] {
        let text = format!("fn main() {{ {constructor} }}");
        let parse = common::parse(&text);
        assert_eq!(parse.diagnostics().is_empty(), !has_marker || has_value);
        if has_marker && !has_value {
            assert_eq!(parse.diagnostics().len(), 1);
            assert_eq!(
                parse.diagnostics()[0].kind,
                DiagnosticKind::ExpectedExpression
            );
        }
        let module = parse.syntax();
        let expr = module
            .syntax()
            .descendants()
            .find_map(StructExpr::cast)
            .unwrap();
        assert_eq!(expr.update_base().is_some(), has_marker);
        assert_eq!(
            expr.update_base().and_then(|base| base.value()).is_some(),
            has_value
        );
        assert_eq!(module.syntax().to_string(), text);
    }
}

#[test]
fn invalid_struct_updates_preserve_following_statements_and_items() {
    for (constructor, has_base, has_value, explicit_fields) in [
        ("Point { .. }", true, false, 0),
        ("Point { .., }", true, false, 0),
        ("Point { ..base, }", true, true, 0),
        ("Point { ..base, ..other }", true, true, 0),
        ("Point { .. ..other }", true, false, 0),
        ("Point { ..base, x: 1, y }", true, true, 0),
        ("Point { x, ..base, y: 1 }", true, true, 1),
        ("Point { ..base x: 1 }", true, true, 0),
        ("Point { ..base x }", true, true, 0),
        ("Point { .., x: 1 }", true, false, 0),
        ("Point { x ..base }", true, true, 1),
        ("Point { ..=base }", false, false, 0),
        ("Point { x, ..=base }", false, false, 1),
        ("Point { ..base, ..=other }", true, true, 0),
    ] {
        let text = format!(
            "fn main() {{ val p = {constructor}; val after = 7; after }} fn later() {{ 1 }}"
        );
        let parse = common::parse(&text);
        assert!(!parse.diagnostics().is_empty(), "{constructor}");
        let module = parse.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let expr = module
            .syntax()
            .descendants()
            .find_map(StructExpr::cast)
            .unwrap();
        assert_eq!(expr.update_base().is_some(), has_base, "{constructor}");
        assert_eq!(
            expr.update_base().and_then(|base| base.value()).is_some(),
            has_value
        );
        assert_eq!(expr.field_list().unwrap().fields().count(), explicit_fields);
        assert_eq!(
            expr.syntax()
                .children()
                .filter_map(StructUpdateBase::cast)
                .count(),
            usize::from(has_base)
        );
        let body = common::first_function(&module).body().unwrap();
        assert_eq!(body.statements().count(), 2, "{constructor}");
        assert!(matches!(
            body.statements().nth(1),
            Some(Stmt::BindingStmt(_))
        ));
        let Some(Expr::PathExpr(tail)) = body.tail_expr() else {
            panic!("lost following tail: {constructor}");
        };
        assert_eq!(tail.name_text().as_deref(), Some("after"));
        let Some(Item::FnDef(later)) = module.items().nth(1) else {
            panic!("lost following function: {constructor}");
        };
        assert_eq!(later.name_text().as_deref(), Some("later"));
    }
}

#[test]
fn struct_update_recovery_leaves_enclosing_delimiters_and_following_source() {
    for expression in [
        "consume(Point { .. })",
        "consume(Point { ..)",
        "[Point { .. }]",
        "[Point { ..]",
        "f\"{Point { .. }}\"",
        "Point { ..;",
        "Point { .. val after_base = 1;",
        "Point { ..base, ;",
    ] {
        let text = format!("fn main() {{ {expression}; val after = 7; after }} fn later() {{ 1 }}");
        let parse = common::parse(&text);
        assert!(!parse.diagnostics().is_empty(), "{expression}");
        let module = parse.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        assert!(
            matches!(body.statements().last(), Some(Stmt::BindingStmt(_))),
            "{expression}"
        );
        let Some(Expr::PathExpr(tail)) = body.tail_expr() else {
            panic!("lost following tail: {expression}");
        };
        assert_eq!(tail.name_text().as_deref(), Some("after"));
        assert_eq!(module.items().count(), 2, "{expression}");
    }
}

#[test]
fn struct_update_marker_does_not_change_range_or_pattern_rest_syntax() {
    let module = common::parse_ok(
        "fn main() { val ranges = [..end, start.., start..=end]; \
         val p = Point { x: start..end, ..(start..end) }; \
         match p { Point { x, .., } => Point { x, ..base } } }",
    );
    assert_eq!(
        module
            .syntax()
            .descendants()
            .filter(|node| node.kind() == SyntaxKind::RangeExpr)
            .count(),
        5
    );
    assert_eq!(
        module
            .syntax()
            .descendants()
            .filter_map(StructUpdateBase::cast)
            .count(),
        2
    );
}
