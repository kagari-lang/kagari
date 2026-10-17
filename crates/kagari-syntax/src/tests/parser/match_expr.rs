use crate::{
    ast::{
        expr::{Expr, MatchExpr, Pattern, PatternBound},
        traits::AstNode,
    },
    kind::SyntaxKind,
    tests::common,
};

#[test]
fn parses_match_expression_with_simple_patterns() {
    let module = common::parse_ok("fn main() { match color { Red => 1, Green => 2, _ => 3 } }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");

    match body.tail_expr().expect("expected tail expression") {
        Expr::MatchExpr(match_expr) => {
            match match_expr.scrutinee().expect("expected scrutinee") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("color")),
                other => panic!("unexpected scrutinee: {other:?}"),
            }

            let arms = match_expr
                .arms()
                .expect("expected arms")
                .arms()
                .collect::<Vec<_>>();

            assert_eq!(arms.len(), 3);

            let first_pattern = arms[0].pattern().expect("expected first pattern");
            assert_eq!(
                first_pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Red")
            );
            match arms[0].expr().expect("expected first expr") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected first arm expr: {other:?}"),
            }

            let second_pattern = arms[1].pattern().expect("expected second pattern");
            assert_eq!(
                second_pattern
                    .path()
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("Green")
            );

            let wildcard = arms[2].pattern().expect("expected wildcard pattern");
            assert!(wildcard.is_wildcard());
        }
        other => panic!("unexpected tail expression: {other:?}"),
    }
}

#[test]
fn struct_pattern_rest_preserves_fields_and_physical_token() {
    for (pattern_text, names, has_rest) in [
        ("Point {}", vec![], false),
        ("Point { x, y: _ }", vec!["x", "y"], false),
        ("Point { .. }", vec![], true),
        ("Point { .., }", vec![], true),
        ("Point { x, .. }", vec!["x"], true),
        ("Point { x, y: _, .., }", vec!["x", "y"], true),
        ("Point { x, /* rest */ .. /* tail */ , }", vec!["x"], true),
        ("Point { x, /* UTF-8: 😀 */ ..,\r\n }", vec!["x"], true),
    ] {
        let text = format!("fn main() {{ match point {{ {pattern_text} => 1 }} }}");
        let module = common::parse_ok(&text);
        assert_eq!(module.syntax().to_string(), text);
        let pattern = module
            .syntax()
            .descendants()
            .find_map(Pattern::cast)
            .expect("expected struct pattern");
        assert!(pattern.is_struct());
        assert_eq!(pattern.has_rest(), has_rest, "{pattern_text}");
        assert_eq!(pattern.range_inclusive(), None);
        assert_eq!(pattern.elements().count(), 0);
        let fields = pattern.fields().collect::<Vec<_>>();
        assert_eq!(
            fields
                .iter()
                .map(|field| field.name().unwrap().text().unwrap())
                .collect::<Vec<_>>(),
            names
        );
        if let Some(first) = fields.first() {
            assert!(first.pattern().is_none(), "shorthand must remain shorthand");
        }
        if let Some(second) = fields.get(1) {
            assert!(second.pattern().unwrap().is_wildcard());
        }
        let rests = pattern
            .syntax()
            .children_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| token.kind() == SyntaxKind::DotDot)
            .collect::<Vec<_>>();
        assert_eq!(rests.len(), usize::from(has_rest));
        if let Some(rest) = rests.first() {
            let start = text.find("..").unwrap();
            assert_eq!(usize::from(rest.text_range().start()), start);
            assert_eq!(usize::from(rest.text_range().end()), start + 2);
            assert_eq!(rest.text(), "..");
        }
    }
}

#[test]
fn struct_pattern_rest_is_local_and_distinct_from_ranges() {
    let text = "fn main() { match value { \
        Outer { point: Point { x, .. }, limit: 1..=MAX } => 1, \
        (Point { .. }, MIN..MAX) => 2, \
        Some(Point { .. }) => 3, \
        (Point { .. }) => 4, \
        Point { .. } | Point { x, .. } => 5, \
        } }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let match_expr = module
        .syntax()
        .descendants()
        .find_map(MatchExpr::cast)
        .unwrap();
    let patterns = match_expr
        .arms()
        .unwrap()
        .arms()
        .map(|arm| arm.pattern().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(patterns.len(), 5);

    let outer = &patterns[0];
    assert!(outer.is_struct());
    assert!(
        !outer.has_rest(),
        "nested rest must not apply to its parent"
    );
    assert_eq!(outer.range_inclusive(), None);
    let fields = outer.fields().collect::<Vec<_>>();
    let nested = fields[0].pattern().unwrap();
    assert!(nested.has_rest());
    assert_eq!(nested.range_inclusive(), None);
    let inclusive = fields[1].pattern().unwrap();
    assert!(!inclusive.has_rest());
    assert_eq!(inclusive.range_inclusive(), Some(true));
    let bounds = inclusive.range_bounds().collect::<Vec<_>>();
    assert!(
        matches!(&bounds[0], PatternBound::Literal(value) if value.text().as_deref() == Some("1"))
    );
    assert!(
        matches!(&bounds[1], PatternBound::Path(path) if path.name_text().as_deref() == Some("MAX"))
    );

    let tuple = &patterns[1];
    assert!(tuple.is_tuple());
    assert!(!tuple.has_rest());
    assert_eq!(tuple.range_inclusive(), None);
    let elements = tuple.elements().collect::<Vec<_>>();
    assert!(elements[0].has_rest());
    let exclusive = &elements[1];
    assert!(!exclusive.has_rest());
    assert_eq!(exclusive.range_inclusive(), Some(false));
    let bounds = exclusive
        .range_bounds()
        .map(|bound| match bound {
            PatternBound::Path(path) => path.name_text().unwrap(),
            PatternBound::Literal(_) => panic!("expected named range bound"),
        })
        .collect::<Vec<_>>();
    assert_eq!(bounds, ["MIN", "MAX"]);

    assert!(patterns[2].is_tuple_struct());
    assert!(patterns[3].is_grouped());
    assert!(patterns[4].is_or());
    for wrapper in &patterns[2..] {
        assert!(!wrapper.has_rest());
        assert_eq!(wrapper.range_inclusive(), None);
        assert!(wrapper.elements().all(|element| element.has_rest()));
    }
}

#[test]
fn struct_pattern_rest_is_shared_by_existing_pattern_contexts() {
    let text = "fn main() { \
        for Point { x, .. } in points {} \
        if val Point { .. } = point {} \
        while val Point { x: _, .., } = point {} \
        match point { Point { .. } => 1 } \
        }";
    let module = common::parse_ok(text);
    assert_eq!(module.syntax().to_string(), text);
    let patterns = module
        .syntax()
        .descendants()
        .filter_map(Pattern::cast)
        .filter(Pattern::is_struct)
        .collect::<Vec<_>>();
    assert_eq!(patterns.len(), 4);
    assert!(patterns.iter().all(Pattern::has_rest));
    assert!(
        patterns
            .iter()
            .all(|pattern| pattern.range_inclusive().is_none())
    );
}

#[test]
fn malformed_struct_pattern_rest_preserves_following_constructs() {
    for pattern_text in [
        "Point { .., .. }",
        "Point { .., x }",
        "Point { .. x }",
        "Point { .. .. }",
        "Point { x, .., y: Point { .. } }",
        "Point { ..,",
        "Point { .., ..,",
        "Point { .., x,",
    ] {
        let text = format!(
            "fn main() {{ match point {{ {pattern_text} => 1, _ => 2 }}; \
             val after = 3; after }} fn later() {{}}"
        );
        let parsed = common::parse(&text);
        assert!(!parsed.diagnostics().is_empty(), "{pattern_text}");
        let module = parsed.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let function = common::first_function(&module);
        let body = function.body().unwrap();
        assert_eq!(body.statements().count(), 2, "{pattern_text}");
        assert!(matches!(body.tail_expr(), Some(Expr::PathExpr(path))
            if path.name_text().as_deref() == Some("after")));
        let later = module.items().nth(1).expect("expected following function");
        assert!(later.syntax().to_string().contains("fn later()"));
        let match_expr = body
            .syntax()
            .descendants()
            .find_map(MatchExpr::cast)
            .unwrap();
        let arms = match_expr.arms().unwrap().arms().collect::<Vec<_>>();
        assert_eq!(arms.len(), 2, "{pattern_text}");
        let pattern = arms[0].pattern().unwrap();
        assert!(pattern.has_rest());
        assert_eq!(pattern.range_inclusive(), None);
        assert_eq!(
            pattern
                .syntax()
                .children_with_tokens()
                .filter_map(|element| element.into_token())
                .filter(|token| token.kind() == SyntaxKind::DotDot)
                .count(),
            1
        );
        assert!(matches!(arms[0].expr(), Some(Expr::Literal(value))
            if value.text().as_deref() == Some("1")));
        assert!(arms[1].pattern().unwrap().is_wildcard());
        assert!(matches!(arms[1].expr(), Some(Expr::Literal(value))
            if value.text().as_deref() == Some("2")));
    }

    for construct in [
        "for Point { .., .., in points {}",
        "if val Point { .., x, = point {}",
        "while val Point { .., .., = point {}",
    ] {
        let text = format!("fn main() {{ {construct} after }}");
        let parsed = common::parse(&text);
        assert!(!parsed.diagnostics().is_empty(), "{construct}");
        let module = parsed.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        assert_eq!(body.statements().count(), 1, "{construct}");
        assert!(matches!(body.tail_expr(), Some(Expr::PathExpr(path))
            if path.name_text().as_deref() == Some("after")));
    }
}
