use crate::{
    ast::{
        expr::{Expr, Pattern},
        item::Item,
        stmt::Stmt,
        traits::AstNode,
    },
    kind::SyntaxKind,
    parser::parse,
    tests::common,
};
use kagari_source::diagnostic::Severity;

#[test]
fn compound_assignments_preserve_lossless_computed_targets() {
    for op in ["=", "+=", "-=", "*=", "/="] {
        let source = format!("fn main() {{ (root())[index()].field {op} // rhs\n value(); }}");
        let parsed = parse(&common::source(&source));
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
        );
        let syntax = parsed.syntax();
        assert_eq!(syntax.syntax().text().to_string(), source);
        let body = common::first_function(&syntax).body().unwrap();
        let Some(Stmt::AssignStmt(statement)) = body.statements().next() else {
            panic!("expected assignment")
        };
        assert!(matches!(statement.target(), Some(Expr::FieldExpr(_))));
        assert!(statement.operator().is_some());
    }
}

#[test]
fn parses_let_return_and_tail_expr_in_block() {
    let module =
        common::parse_ok("fn main() -> i32 { var value: i32 = 1; value = 2; return; value }");

    let function = common::first_function(&module);

    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    assert_eq!(statements.len(), 3);

    match &statements[0] {
        Stmt::BindingStmt(stmt) => {
            assert!(stmt.is_var());
            assert_eq!(
                stmt.pattern()
                    .and_then(|pattern| pattern.path())
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("value")
            );
            assert_eq!(
                stmt.ty().and_then(|ty| ty.name_text()).as_deref(),
                Some("i32")
            );
            match stmt.initializer().expect("expected initializer") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("1")),
                other => panic!("unexpected binding initializer: {other:?}"),
            }
        }
        other => panic!("unexpected first statement: {other:?}"),
    }

    match &statements[1] {
        Stmt::AssignStmt(stmt) => {
            match stmt.target().expect("expected assignment target") {
                Expr::PathExpr(path) => {
                    assert_eq!(path.name_text().as_deref(), Some("value"));
                }
                other => panic!("unexpected assignment target: {other:?}"),
            }
            match stmt.value().expect("expected assigned value") {
                Expr::Literal(literal) => assert_eq!(literal.text().as_deref(), Some("2")),
                other => panic!("unexpected assigned value: {other:?}"),
            }
        }
        other => panic!("unexpected second statement: {other:?}"),
    }

    match &statements[2] {
        Stmt::ReturnStmt(stmt) => assert!(stmt.expr().is_none()),
        other => panic!("unexpected third statement: {other:?}"),
    }

    match body.tail_expr().expect("expected tail expression") {
        Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("value")),
        other => panic!("unexpected tail expr: {other:?}"),
    }
}

#[test]
fn parses_val_binding_without_initializer() {
    let module = common::parse_ok("fn main() { val answer: i32; }");
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    match &statements[0] {
        Stmt::BindingStmt(stmt) => {
            assert!(stmt.is_val());
            assert_eq!(
                stmt.pattern()
                    .and_then(|pattern| pattern.path())
                    .and_then(|path| path.name_text())
                    .as_deref(),
                Some("answer")
            );
            assert_eq!(
                stmt.ty().and_then(|ty| ty.name_text()).as_deref(),
                Some("i32")
            );
            assert!(stmt.initializer().is_none());
        }
        other => panic!("unexpected first statement: {other:?}"),
    }
}

#[test]
fn parses_field_and_index_assignment_targets() {
    let module = common::parse_ok(
        r#"
fn main() {
    point.inner.x = 1;
    values[0] = 2;
}
"#,
    );
    let function = common::first_function(&module);
    let body = function.body().expect("expected function body");
    let statements: Vec<_> = body.statements().collect();

    match &statements[0] {
        Stmt::AssignStmt(stmt) => match stmt.target().expect("expected target") {
            Expr::FieldExpr(field) => {
                assert_eq!(field.name_text().as_deref(), Some("x"));
            }
            other => panic!("unexpected field assignment target: {other:?}"),
        },
        other => panic!("unexpected first statement: {other:?}"),
    }

    match &statements[1] {
        Stmt::AssignStmt(stmt) => match stmt.target().expect("expected target") {
            Expr::IndexExpr(index) => match index.receiver().expect("expected receiver") {
                Expr::PathExpr(path) => assert_eq!(path.name_text().as_deref(), Some("values")),
                other => panic!("unexpected index receiver: {other:?}"),
            },
            other => panic!("unexpected index assignment target: {other:?}"),
        },
        other => panic!("unexpected second statement: {other:?}"),
    }
}

#[test]
fn binding_patterns_preserve_direct_children_and_whole_initializer() {
    for (keyword, pattern_text, annotation, initializer_text) in [
        ("val", "name", None, "incoming"),
        ("val", "_", None, "work()"),
        ("var", "(left, right)", Some("(i32, i32)"), "make_pair()"),
        ("val", "Point { x, y: _ }", Some("Point"), "point"),
        ("var", "Point { x, .. }", None, "point"),
        ("val", "Point { .., }", Some("Point"), "point"),
        ("val", "(id, Point { x: coordinate, .. })", None, "payload"),
        ("val", "1", None, "incoming"),
        ("val", "1..=MAX", None, "incoming"),
        ("val", "Some(item)", None, "incoming"),
    ] {
        let annotation_text = annotation.map(|ty| format!(": {ty}")).unwrap_or_default();
        let text = format!(
            "// UTF-8: 😀\r\nfn main() {{ {keyword} {pattern_text}{annotation_text} = {initializer_text}; }}"
        );
        let module = common::parse_ok(&text);
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        let Some(Stmt::BindingStmt(binding)) = body.statements().next() else {
            panic!("expected declaration");
        };
        assert_eq!(binding.is_var(), keyword == "var");
        assert_eq!(binding.is_val(), keyword == "val");
        let pattern = binding.pattern().expect("expected direct pattern");
        assert_eq!(pattern.syntax().to_string().trim_end(), pattern_text);
        assert_eq!(pattern.is_wildcard(), pattern_text == "_");
        assert!(!pattern.is_or());
        let start = text.find(pattern_text).unwrap();
        assert_eq!(usize::from(pattern.syntax().text_range().start()), start);
        let last_token = pattern
            .syntax()
            .descendants_with_tokens()
            .filter_map(|element| element.into_token())
            .filter(|token| !token.kind().is_trivia())
            .last()
            .unwrap();
        assert_eq!(
            usize::from(last_token.text_range().end()),
            start + pattern_text.len()
        );
        let children = binding.syntax().children().collect::<Vec<_>>();
        assert_eq!(&children[0], pattern.syntax());
        assert_eq!(children.len(), if annotation.is_some() { 3 } else { 2 });
        assert_eq!(
            binding
                .ty()
                .map(|ty| ty.syntax().to_string().trim().to_owned()),
            annotation.map(str::to_owned)
        );
        if let Some(ty) = binding.ty() {
            assert_eq!(&children[1], ty.syntax());
        }
        let initializer = binding.initializer().expect("expected whole initializer");
        assert_eq!(initializer.syntax().to_string().trim(), initializer_text);
        assert_eq!(children.last().unwrap(), initializer.syntax());
        assert_eq!(
            binding.syntax().children().filter_map(Expr::cast).count(),
            1
        );
    }
}

#[test]
fn binding_patterns_preserve_nested_struct_and_tuple_shapes() {
    let module = common::parse_ok(
        "fn main() { var (id, Point { x: coordinate, .. }): (i32, Point) = payload; }",
    );
    let body = common::first_function(&module).body().unwrap();
    let Some(Stmt::BindingStmt(binding)) = body.statements().next() else {
        panic!("expected declaration");
    };
    let pattern = binding.pattern().unwrap();
    assert!(pattern.is_tuple());
    let elements = pattern.elements().collect::<Vec<_>>();
    assert_eq!(elements.len(), 2);
    assert_eq!(
        elements[0].path().unwrap().name_text().as_deref(),
        Some("id")
    );
    let point = &elements[1];
    assert!(point.is_struct());
    assert!(point.has_rest());
    assert_eq!(point.path().unwrap().name_text().as_deref(), Some("Point"));
    let fields = point.fields().collect::<Vec<_>>();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name().unwrap().text().as_deref(), Some("x"));
    assert_eq!(
        fields[0]
            .pattern()
            .unwrap()
            .path()
            .unwrap()
            .name_text()
            .as_deref(),
        Some("coordinate")
    );
    let types = binding
        .ty()
        .unwrap()
        .tuple_type()
        .unwrap()
        .element_types()
        .collect::<Vec<_>>();
    assert_eq!(types.len(), 2);
    assert_eq!(types[0].name_text().as_deref(), Some("i32"));
    assert_eq!(types[1].name_text().as_deref(), Some("Point"));
    assert!(matches!(binding.initializer(), Some(Expr::PathExpr(path))
        if path.name_text().as_deref() == Some("payload")));
}

#[test]
fn binding_patterns_allow_nested_but_not_top_level_alternatives() {
    for pattern_text in [
        "(left | right)",
        "(left | right, _)",
        "Some(left | right)",
        "Point { x: left | right, .. }",
    ] {
        let text = format!("fn main() {{ val {pattern_text} = incoming; }}");
        let module = common::parse_ok(&text);
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        let Some(Stmt::BindingStmt(binding)) = body.statements().next() else {
            panic!("expected declaration");
        };
        let pattern = binding.pattern().unwrap();
        assert!(!pattern.is_or());
        assert_eq!(
            pattern
                .syntax()
                .descendants()
                .filter_map(Pattern::cast)
                .filter(Pattern::is_or)
                .count(),
            1
        );
        if pattern_text == "(left | right)" {
            assert!(pattern.is_grouped());
            assert!(pattern.elements().next().unwrap().is_or());
        }
    }

    for pattern_text in ["left | right", "left | right | other", "| left"] {
        let text =
            format!("fn main() {{ val {pattern_text}: Pair = incoming; val next = 2; next }}");
        let parsed = common::parse(&text);
        assert!(!parsed.diagnostics().is_empty(), "{pattern_text}");
        let module = parsed.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        let statements = body.statements().collect::<Vec<_>>();
        assert_eq!(statements.len(), 2);
        let Stmt::BindingStmt(binding) = &statements[0] else {
            panic!("expected declaration");
        };
        assert!(!binding.pattern().unwrap().is_or());
        assert_eq!(binding.ty().unwrap().name_text().as_deref(), Some("Pair"));
        assert!(
            binding
                .syntax()
                .children()
                .any(|node| node.kind() == SyntaxKind::Error)
        );
        assert!(matches!(binding.initializer(), Some(Expr::PathExpr(path))
            if path.name_text().as_deref() == Some("incoming")));
        assert!(matches!(body.tail_expr(), Some(Expr::PathExpr(path))
            if path.name_text().as_deref() == Some("next")));
    }
}

#[test]
fn binding_patterns_without_initializers_remain_recoverable_syntax() {
    for pattern_text in ["name", "_", "(left, right)", "Point { x, .. }"] {
        let text = format!("fn main() {{ val {pattern_text}; val next = 2; next }}");
        let module = common::parse_ok(&text);
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        let Some(Stmt::BindingStmt(binding)) = body.statements().next() else {
            panic!("expected declaration");
        };
        assert_eq!(
            binding.pattern().unwrap().syntax().to_string().trim(),
            pattern_text
        );
        assert!(binding.initializer().is_none());
        assert!(binding.ty().is_none());
        assert_eq!(binding.syntax().children().count(), 1);
        assert_eq!(body.statements().count(), 2);
    }
}

#[test]
fn malformed_binding_patterns_preserve_following_declarations() {
    for (declaration, initializer_text) in [
        ("val = incoming;", Some("incoming")),
        ("val (left, = incoming;", Some("incoming")),
        ("val Point { x, .., = incoming;", Some("incoming")),
        ("val Point { .., .. } = incoming;", Some("incoming")),
        ("val Point { .., x } = incoming;", Some("incoming")),
        ("val (left, right): = incoming;", Some("incoming")),
        ("val (left, right) = ;", None),
        ("val ;", None),
        ("val (left, right) = incoming", Some("incoming")),
    ] {
        let text = format!("fn main() {{ {declaration} val next = 2; next }} fn later() {{}}");
        let parsed = common::parse(&text);
        assert!(!parsed.diagnostics().is_empty(), "{declaration}");
        let module = parsed.syntax();
        assert_eq!(module.syntax().to_string(), text);
        let body = common::first_function(&module).body().unwrap();
        let statements = body.statements().collect::<Vec<_>>();
        assert_eq!(statements.len(), 2, "{declaration}");
        let Stmt::BindingStmt(binding) = &statements[0] else {
            panic!("expected recovered declaration");
        };
        assert!(binding.pattern().is_some());
        assert_eq!(
            binding
                .initializer()
                .map(|expr| expr.syntax().to_string().trim().to_owned()),
            initializer_text.map(str::to_owned),
            "{declaration}"
        );
        let Stmt::BindingStmt(next) = &statements[1] else {
            panic!("expected following declaration");
        };
        assert_eq!(
            next.pattern()
                .unwrap()
                .path()
                .unwrap()
                .name_text()
                .as_deref(),
            Some("next")
        );
        assert!(matches!(body.tail_expr(), Some(Expr::PathExpr(path))
            if path.name_text().as_deref() == Some("next")));
        assert!(matches!(module.items().nth(1), Some(Item::FnDef(function))
            if function.name_text().as_deref() == Some("later")));
    }
}

#[test]
fn rejects_invalid_let_binding() {
    let parse = common::parse("fn main() { let mut value = 1; }");

    assert_eq!(
        parse.diagnostics()[0].severity,
        Severity::Error,
        "expected an error, got {:?}",
        parse.diagnostics()
    );
}
