//! Stable recovered grammar roles, source storage and retained module ownership.

use kagari_source::{diagnostic::DiagnosticKind, source::SourceFile};
use kagari_syntax::{
    ast::{
        site::{RecoveryPart, Required},
        views::{ExprView, ItemView},
        visit::{ElementRef, NodeRef},
    },
    parser::{ParseOptions, parse},
};

#[test]
fn missing_condition_and_else_body_cannot_shift_field_roles() {
    for (text, missing_condition, has_else) in [
        ("fn f(){if {}}", true, false),
        ("fn f(){if {} else", true, true),
        ("fn f(){if ready {}}", false, false),
        ("fn f(){if {true} {}}", false, false),
        ("fn f(){if {true} {} else", false, true),
        ("fn f(){if ready {} else", false, true),
    ] {
        let parsed = parse(
            &SourceFile::new("roles", text),
            ParseOptions::default(),
            &Default::default(),
        )
        .expect("fresh cancellation token");
        let root = parsed.module();
        let ItemView::FnDef(function) = root.items().next().unwrap() else {
            panic!()
        };
        let ExprView::IfExpr(conditional) = function.body().unwrap().tail_expr().unwrap() else {
            panic!()
        };
        assert_eq!(
            matches!(conditional.data().condition, Required::Missing { .. }),
            missing_condition
        );
        assert!(matches!(conditional.data().then_body, Required::Present(_)));
        if text.contains("true") {
            assert!(matches!(
                conditional.condition(),
                Some(ExprView::BlockExpr(_))
            ));
        }
        assert_eq!(conditional.data().else_clause.is_some(), has_else);
        if has_else {
            assert!(
                matches!(conditional.data().else_clause.as_ref().unwrap().body, Required::Missing { at } if at as usize == text.len())
            );
        }
    }
}

#[test]
fn continued_lists_keep_recovery_positions_and_authored_token_order() {
    fn walk(node: NodeRef<'_>, text: &mut String) {
        for element in node.elements() {
            match element {
                ElementRef::Node(child) => walk(child, text),
                ElementRef::Token(token) if !token.lexical_kind().is_trivia() => {
                    text.push_str(token.text());
                }
                ElementRef::Operator(operator) => text.push_str(operator.text()),
                _ => {}
            }
        }
    }

    for (text, entry, part) in [
        ("fn f(){(a,b @@@,c)}", 1, RecoveryPart::Separator),
        ("fn f(){[a,b @@@,c]}", 1, RecoveryPart::Separator),
        ("fn f(x: (A,B @@@,C)){}", 1, RecoveryPart::Separator),
        (
            "fn f(){val (a,b @@@,c) = input;}",
            1,
            RecoveryPart::Separator,
        ),
        (
            "trait T {fn f(self, x: A @@@, y: B);}",
            1,
            RecoveryPart::Separator,
        ),
        ("trait T { @@@ }", 0, RecoveryPart::Element),
        ("trait T { @@@ fn f(); }", 0, RecoveryPart::Element),
        ("trait T { fn f(); @@@ fn g(); }", 1, RecoveryPart::Element),
        ("trait T { fn f(); @@@ }", 1, RecoveryPart::Element),
        ("impl T { @@@ }", 0, RecoveryPart::Element),
        ("impl T { @@@ fn f(){} }", 0, RecoveryPart::Element),
        ("impl T { fn f(){} @@@ fn g(){} }", 1, RecoveryPart::Element),
        ("impl T { fn f(){} @@@ }", 1, RecoveryPart::Element),
    ] {
        let parsed = parse(
            &SourceFile::new("continued-list", text),
            ParseOptions::default(),
            &Default::default(),
        )
        .expect("fresh cancellation token");
        assert!(!parsed.diagnostics().is_empty());
        let module = parsed.module();
        let mut recoveries = Vec::new();
        for node in module.descendants() {
            let skipped = match node {
                NodeRef::TupleExpr(node) => node.unexpected().collect::<Vec<_>>(),
                NodeRef::ArrayExpr(node) => node.unexpected().collect(),
                NodeRef::Type(node) => node.unexpected().collect(),
                NodeRef::Pattern(node) => node.unexpected().collect(),
                NodeRef::ParamList(node) => node.unexpected().collect(),
                NodeRef::TraitDef(node) => node.unexpected().collect(),
                NodeRef::ImplBlock(node) => node.unexpected().collect(),
                _ => Vec::new(),
            };
            recoveries.extend(skipped);
        }
        assert_eq!(recoveries.len(), 1, "{text}");
        assert_eq!(recoveries[0].text(), "@@@");
        assert_eq!(recoveries[0].position().entry, entry, "{text}");
        assert_eq!(recoveries[0].position().part, part);
        if part == RecoveryPart::Element {
            assert_eq!(recoveries[0].position().field, "members", "{text}");
        }
        let mut walked = String::new();
        walk(NodeRef::ModuleBody(module.body()), &mut walked);
        let lexical = parsed
            .tokens()
            .filter(|token| !token.lexical_kind().is_trivia())
            .map(|token| token.text())
            .collect::<String>();
        assert_eq!(walked, lexical, "{text}");
    }
}

#[test]
fn missing_list_elements_trailing_separators_and_skipped_input_remain_distinct() {
    for text in ["fn f(){f(a,,b,)}", "fn f(){f(a @@@,b)}", "fn f(){f(a"] {
        let parsed = parse(
            &SourceFile::new("arguments", text),
            ParseOptions::default(),
            &Default::default(),
        )
        .expect("fresh cancellation token");
        assert!(!parsed.diagnostics().is_empty());
        let root = parsed.module();
        let ItemView::FnDef(function) = root.items().next().unwrap() else {
            panic!()
        };
        let ExprView::CallExpr(call) = function.body().unwrap().tail_expr().unwrap() else {
            panic!()
        };
        if text.contains(",,") {
            let entries = &call.data().arguments.entries;
            assert_eq!(entries.len(), 3);
            assert!(matches!(entries[1].element, Required::Missing { .. }));
            assert!(
                entries
                    .iter()
                    .all(|entry| matches!(entry.separator, Some(Required::Present(_))))
            );
            assert_eq!(call.args().count(), 2);
        } else if text.contains('@') {
            assert_eq!(call.args().count(), 2);
            let skipped = call.unexpected().collect::<Vec<_>>();
            assert_eq!(skipped.len(), 1);
            assert_eq!(skipped[0].text(), "@@@");
            assert_eq!(skipped[0].position().field, "arguments");
            assert_eq!(skipped[0].position().entry, 0);
            assert_eq!(skipped[0].position().part, RecoveryPart::Separator);
        } else {
            assert!(
                matches!(call.data().close, Required::Missing { at } if at as usize == text.len())
            );
        }
        assert_eq!(
            parsed
                .tokens()
                .map(|token| token.text())
                .collect::<String>(),
            text
        );
    }
}

#[test]
fn source_backing_trivia_boundaries_and_trivia_only_files_are_lossless() {
    for text in [
        "// initial\r\nfn f(){1} // trailing\r\n\r\n// next\r\nfn g(){2}",
        "fn f(){1} /* atomic\n comment */\n\nfn g(){2}",
        "// 中文😀\r\n /* nested /* retained */ */ \n",
    ] {
        let source = SourceFile::new("trivia", text);
        let parsed = parse(&source, ParseOptions::default(), &Default::default())
            .expect("fresh cancellation token");
        assert!(
            parsed.diagnostics().is_empty(),
            "{:?}",
            parsed.diagnostics()
        );
        assert_eq!(parsed.source().text().as_ptr(), source.text().as_ptr());
        let mut offset = 0;
        for token in parsed.tokens() {
            assert_eq!(token.span().start, offset);
            offset = token.span().end;
        }
        assert_eq!(offset, text.len());
        assert_eq!(
            parsed
                .tokens()
                .map(|token| token.text())
                .collect::<String>(),
            text
        );
        let root = parsed.module();
        let functions = root.items().collect::<Vec<_>>();
        if let [ItemView::FnDef(first), ItemView::FnDef(second)] = functions.as_slice() {
            let boundary = if text.contains("atomic") {
                text.find("*/").unwrap() + 2
            } else {
                text.find("\r\n\r\n").unwrap() + 2
            };
            assert_eq!(first.full_span().end, boundary);
            assert_eq!(second.full_span().start, boundary);
            assert_eq!(first.body_span().start, text.find("fn f").unwrap());
        }
    }
}

#[test]
fn lexical_coordinates_preserve_utf8_across_storage_widths() {
    for prefix in ["", &"中😀".repeat(10_000)] {
        let text = format!("/*{prefix}*/\r\nfn following(){{1}} ");
        let parsed = parse(
            &SourceFile::new("offsets", text.clone()),
            ParseOptions::default(),
            &Default::default(),
        )
        .expect("fresh cancellation token");
        assert!(parsed.diagnostics().is_empty());
        let module = parsed.module();
        let ItemView::FnDef(function) = module.items().next().unwrap() else {
            panic!()
        };
        assert_eq!(
            function.body_span().start,
            text.find("fn following").unwrap()
        );
        assert_eq!(
            function.name().unwrap().body_span().start,
            text.find("following").unwrap()
        );
        assert_eq!(
            parsed
                .tokens()
                .map(|token| token.text())
                .collect::<String>(),
            text
        );
    }
}

#[test]
fn retained_inline_view_outlives_root_and_rejects_foreign_parse_locators() {
    let source = SourceFile::new(
        "modules",
        "// 中文😀\r\nmod a { //! A\r\nfn f(){} mod b{fn g(){}} }",
    );
    let parsed = parse(&source, ParseOptions::default(), &Default::default())
        .expect("fresh cancellation token");
    let root = parsed.module();
    let ItemView::ModuleDef(declaration) = root.items().next().unwrap() else {
        panic!()
    };
    let child = declaration.body().unwrap();
    let location = child.location();
    assert_eq!(child.module_documentation(), "A");
    assert!(
        location
            .resolve(
                &parse(&source, ParseOptions::default(), &Default::default())
                    .expect("fresh cancellation token")
            )
            .is_none()
    );
    assert!(location.resolve(&parsed).is_some());
    drop(root);
    drop(parsed);
    assert_eq!(child.items().count(), 2);
    assert_eq!(child.source().text().as_ptr(), source.text().as_ptr());
    assert!(child.text().contains("fn g()"));
}

#[test]
fn iterative_expression_chains_stop_before_unbounded_recursive_destruction() {
    for expression in [
        format!("x{}", ".field".repeat(4096)),
        format!("x{}", "+x".repeat(4096)),
    ] {
        let source = SourceFile::new("chain", format!("fn f(){{{expression}}}"));
        let parsed = parse(&source, ParseOptions::default(), &Default::default())
            .expect("fresh cancellation token");
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
            diagnostic.kind,
            DiagnosticKind::CompileLimitExceeded {
                resource: "syntax tree depth",
                ..
            }
        )));
        assert_eq!(
            parsed
                .tokens()
                .map(|token| token.text())
                .collect::<String>(),
            source.text()
        );
        assert!(
            parsed
                .module()
                .body()
                .unexpected()
                .any(|suffix| suffix.position().field == "unparsed_suffix")
        );
        drop(parsed);
    }
}
