use crate::ast::visit::{ElementRef, NodeRef};

use crate::ast::views::ItemView;

use crate::{
    ast::{borrow::AstNode, item::VisibilityKind},
    kind::SyntaxKind,
    tests::common,
};
use kagari_source::diagnostic::DiagnosticKind;

#[test]
fn static_declarations_retain_direct_children_and_prefixes() {
    for (prefix, visibility) in [
        ("", VisibilityKind::Private),
        ("pub ", VisibilityKind::Public),
        ("pub(super) ", VisibilityKind::PublicSuper),
        ("pub(crate) ", VisibilityKind::PublicCrate),
    ] {
        for binding in ["val", "var"] {
            for attributes in ["", "#[tag(value = 7)] #[other] "] {
                let text = format!(
                    "// UTF-8: 😀\r\n{attributes}{prefix}static {binding} cache: Vec<i32> = make(1 + 2);"
                );
                let module = common::parse_ok(&text);
                assert_eq!(module.text().to_owned(), text);
                let item = module.items().next().unwrap();
                let ItemView::StaticDef(def) = &item else {
                    panic!("expected static, got {item:?}");
                };
                assert_eq!(def.name().unwrap().text().as_deref(), Some("cache"));
                assert_eq!(def.visibility(), visibility);
                assert_eq!(def.is_var(), binding == "var");
                assert_eq!(
                    def.ty().unwrap().source_text().to_owned().trim(),
                    "Vec<i32>"
                );
                assert_eq!(
                    def.initializer().unwrap().source_text().to_owned().trim(),
                    "make(1 + 2)"
                );
                assert_eq!(
                    def.attributes().count(),
                    if attributes.is_empty() { 0 } else { 2 }
                );
                assert_eq!(item.attributes().count(), def.attributes().count());
            }
        }
    }
}

#[test]
fn static_declarations_belong_to_inline_modules() {
    let text = "mod state { static var count: i32 = 0; mod nested { #[tag] pub static val cache: [i32] = [1, 2]; } } static val callback: fn(i32) -> i32 = |value| value;";
    let module = common::parse_ok(text);
    let items: Vec<_> = module.items().collect();
    assert_eq!(items.len(), 2);
    assert!(matches!(items[0], ItemView::ModuleDef(_)));
    assert!(matches!(items[1], ItemView::StaticDef(_)));
    let definitions: Vec<_> = NodeRef::ModuleBody(module.body())
        .descendants()
        .filter_map(|node| match node {
            NodeRef::StaticDef(node) => Some(node),
            _ => None,
        })
        .collect();
    assert_eq!(definitions.len(), 3);
    assert_eq!(
        definitions
            .iter()
            .map(|def| def.name().unwrap().text().unwrap())
            .collect::<Vec<_>>(),
        ["count", "cache", "callback"]
    );
    let ItemView::ModuleDef(state) = items[0] else {
        panic!("state module")
    };
    let state_body = state.body().unwrap();
    assert!(matches!(
        state_body.items().next(),
        Some(ItemView::StaticDef(_))
    ));
    assert_eq!(module.text().to_owned(), text);
}

#[test]
fn static_malformed_declarations_recover_before_following_items() {
    for (broken, diagnostic) in [
        (
            "static count: i32 = 0;",
            DiagnosticKind::ExpectedBindingKeyword,
        ),
        ("static val : i32 = 0;", DiagnosticKind::ExpectedBindingName),
        ("static val count = 0;", DiagnosticKind::ExpectedType),
        ("static val count: = 0;", DiagnosticKind::ExpectedType),
        (
            "static val count: i32;",
            DiagnosticKind::ExpectedAssignmentOperator,
        ),
        (
            "static val count: i32 = ;",
            DiagnosticKind::ExpectedExpression,
        ),
        (
            "static val count: i32 = 0",
            DiagnosticKind::ExpectedStatementTerminator,
        ),
        (
            "pub static mut count: i32 = 0;",
            DiagnosticKind::ExpectedBindingKeyword,
        ),
        (
            "static val (a, b): (i32, i32) = (1, 2);",
            DiagnosticKind::ExpectedBindingName,
        ),
        (
            "static val count<T>: T = make();",
            DiagnosticKind::ExpectedType,
        ),
        ("static val", DiagnosticKind::ExpectedBindingName),
        (
            "static val count: i32 = 0 extra;",
            DiagnosticKind::ExpectedStatementTerminator,
        ),
        (
            "static val count: i32 =",
            DiagnosticKind::ExpectedExpression,
        ),
    ] {
        let text = format!("mod state {{ {broken} static var next: i32 = 1; fn after() {{}} }}");
        let parse = common::parse(&text);
        assert!(
            parse
                .diagnostics()
                .iter()
                .any(|error| error.kind == diagnostic),
            "{broken}: {:?}",
            parse.diagnostics()
        );
        let module = parse.module();
        assert_eq!(module.text().to_owned(), text);
        let definitions: Vec<_> = NodeRef::ModuleBody(module.body())
            .descendants()
            .filter_map(|node| match node {
                NodeRef::StaticDef(node) => Some(node),
                _ => None,
            })
            .collect();
        assert_eq!(definitions.len(), 2, "{broken}");
        for skipped in definitions[0].unexpected() {
            assert_eq!(skipped.position().field, "terminator", "{broken}");
        }
        assert_eq!(
            definitions[1].name().unwrap().text().as_deref(),
            Some("next"),
            "{broken}"
        );
        assert_eq!(
            definitions[1]
                .initializer()
                .unwrap()
                .source_text()
                .to_owned()
                .trim(),
            "1"
        );
        assert_eq!(
            NodeRef::ModuleBody(module.body())
                .descendants()
                .filter(|node| node.kind() == SyntaxKind::FnDef)
                .count(),
            1,
            "{broken}"
        );
    }
}

#[test]
fn static_malformed_headers_preserve_following_functions_and_methods() {
    for broken in [
        "static val count:",
        "static mut count: i32 = { 0 };",
        "static val (a, b): (i32, i32) = (1, 2);",
    ] {
        for (prefix, suffix, kind) in [
            ("", "fn next() {}", SyntaxKind::FnDef),
            ("trait T {", "fn next(self); }", SyntaxKind::MethodDef),
            ("impl S {", "fn next(self) {} }", SyntaxKind::MethodDef),
        ] {
            let text = format!("{prefix} {broken} {suffix}");
            let parse = common::parse(&text);
            assert!(!parse.diagnostics().is_empty(), "{text}");
            let module = parse.module();
            assert_eq!(module.text().to_owned(), text);
            assert_error_tokens(NodeRef::ModuleBody(module.body()));
            assert_eq!(
                NodeRef::ModuleBody(module.body())
                    .descendants()
                    .filter(|node| node.kind() == kind)
                    .count(),
                1,
                "{text}: {:?}",
                parse.diagnostics()
            );
        }
    }
}

#[test]
fn static_recovery_accessors_do_not_invent_missing_parts() {
    for text in [
        "static val",
        "static var count: i32",
        "static val count = 0;",
    ] {
        let parse = common::parse(text);
        assert!(!parse.diagnostics().is_empty());
        let module = parse.module();
        let ItemView::StaticDef(def) = module.items().next().unwrap() else {
            panic!("expected recovered static");
        };
        assert_eq!(def.source_text().to_owned(), text);
        if text == "static val" {
            assert!(def.name().and_then(|name| name.text()).is_none());
            assert!(def.ty().is_none());
            assert!(def.initializer().is_none());
        } else if text == "static var count: i32" {
            assert!(def.ty().is_some());
            assert!(def.initializer().is_none());
        } else {
            assert!(def.ty().is_none());
            assert!(def.initializer().is_some());
        }
    }
}

#[test]
fn static_local_and_associated_forms_are_errors_without_losing_following_members() {
    for (prefix, suffix, follower) in [
        ("fn outer() {", "val next = 1; }", SyntaxKind::BindingStmt),
        (
            "fn outer() { val closure = || {",
            "val next = 1; }; }",
            SyntaxKind::BindingStmt,
        ),
        ("trait T {", "fn next(self); }", SyntaxKind::MethodDef),
        ("impl S {", "fn next(self) {} }", SyntaxKind::MethodDef),
        (
            "impl T for S {",
            "fn next(self) {} }",
            SyntaxKind::MethodDef,
        ),
    ] {
        for declaration in [
            "static val count: i32 = { 0 };",
            "#[tag] pub(crate) static var count: i32 = 0;",
        ] {
            let text = format!("{prefix} {declaration} {suffix} static val valid: i32 = 1;");
            let parse = common::parse(&text);
            assert_eq!(
                parse.diagnostics().len(),
                1,
                "{text}: {:?}",
                parse.diagnostics()
            );
            assert_eq!(parse.diagnostics()[0].kind, DiagnosticKind::UnexpectedToken);
            let module = parse.module();
            assert_eq!(module.text().to_owned(), text);
            assert_error_tokens(NodeRef::ModuleBody(module.body()));
            let definitions: Vec<_> = NodeRef::ModuleBody(module.body())
                .descendants()
                .filter_map(|node| match node {
                    NodeRef::StaticDef(node) => Some(node),
                    _ => None,
                })
                .collect();
            assert_eq!(definitions.len(), 1, "{text}");
            assert_eq!(
                definitions[0].name().unwrap().text().as_deref(),
                Some("valid")
            );
            assert!(
                NodeRef::ModuleBody(module.body())
                    .descendants()
                    .any(|node| node.kind() == follower),
                "{text}"
            );
        }
    }
}

fn assert_error_tokens(root: NodeRef<'_>) {
    for node in root.descendants() {
        if let NodeRef::Error(_) = node {
            let expected = node.tokens().map(|token| token.span()).collect::<Vec<_>>();
            let actual = node
                .elements()
                .into_iter()
                .map(|element| {
                    let ElementRef::Token(token) = element else {
                        panic!("rejected declarations retain tokens, not typed children");
                    };
                    token.span()
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "rejected declaration lost tokens");
        }
    }
}
