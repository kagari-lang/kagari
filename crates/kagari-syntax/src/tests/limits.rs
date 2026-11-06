use crate::ast::site::Required;
use crate::ast::visit::NodeRef;

use crate::parser::{ParseLimits, ParseMode, ParseOptions, parse};

use {
    kagari_common::cancellation::CancellationToken,
    kagari_source::{diagnostic::DiagnosticKind, source::SourceFile},
};

#[test]
fn exhausted_header_recovery_does_not_consume_following_tokens() {
    for text in [
        "pub(/<=type A;",
        "#[tag(,)] type A;",
        "#[tag(,)] impl A {}",
        "trait T { #[tag(,)] type A; }",
        "use a { b };",
    ] {
        for mode in [ParseMode::Source, ParseMode::Declarations] {
            let parsed = parse(
                &SourceFile::new("prefix-limit", text),
                ParseOptions {
                    mode,
                    limits: ParseLimits {
                        max_diagnostics: 0,
                        ..Default::default()
                    },
                },
                &Default::default(),
            )
            .unwrap();
            let module = parsed.module();
            let missing = NodeRef::ModuleBody(module.body())
                .descendants()
                .find_map(|node| match node {
                    NodeRef::TypeAlias(node) => {
                        Some(matches!(node.data().keyword, Required::Missing { .. }))
                    }
                    NodeRef::OpaqueTypeDef(node) => {
                        Some(matches!(node.data().keyword, Required::Missing { .. }))
                    }
                    NodeRef::ImplBlock(node) => {
                        Some(matches!(node.data().keyword, Required::Missing { .. }))
                    }
                    NodeRef::AssociatedType(node) => {
                        Some(matches!(node.data().keyword, Required::Missing { .. }))
                    }
                    NodeRef::UseTreeList(node) => {
                        Some(matches!(node.data().open, Required::Missing { .. }))
                    }
                    _ => None,
                });
            assert_eq!(missing, Some(true), "{text}: {mode:?}");
            assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(
                diagnostic.kind,
                DiagnosticKind::CompileLimitExceeded { .. }
            )));
            let suffix = module
                .body()
                .unexpected()
                .find(|part| part.position().field == "unparsed_suffix")
                .unwrap();
            assert!(suffix.text().contains(if text.starts_with("use") {
                "{"
            } else if text.contains("impl") {
                "impl"
            } else {
                "type"
            }));
        }
    }
}

#[test]
fn parser_diagnostic_budget_stops_recovery_and_preserves_lossless_suffix() {
    for prefix in ["", "mod nested {", "trait T {", "impl T {", "fn broken() {"] {
        let text = format!(
            "fn good() -> i32 {{ 42 }} {prefix} {} // 中文 😀\r\nfn later() {{}}",
            "@ ".repeat(500)
        );
        let source = SourceFile::new("limited.kgr", text);
        for max_diagnostics in [0, 1, 7] {
            let parsed = parse(
                &source,
                ParseOptions {
                    limits: ParseLimits {
                        max_diagnostics,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
            assert_eq!(parsed.diagnostics().len(), max_diagnostics + 1);
            let last = parsed.diagnostics().last().unwrap();
            assert_eq!(
                last.kind,
                DiagnosticKind::CompileLimitExceeded {
                    resource: "parser diagnostics",
                    limit: max_diagnostics
                }
            );
            let span = last.span.unwrap();
            assert_eq!(&source.text()[span.start..span.end], "@");
            assert_eq!(parsed.module().text().to_owned(), source.text());
            assert!(
                !parsed
                    .module()
                    .descendants()
                    .any(|node| node.kind() == crate::kind::SyntaxKind::FnDef
                        && node.text().to_string().contains("later")),
                "the suffix must not be parsed"
            );
        }
    }
}

#[test]
fn parser_budget_allows_exact_limit_and_valid_source_at_zero() {
    for (text, limit) in [("fn good() -> i32 { 42 }", 0), ("@", 1)] {
        let parsed = parse(
            &SourceFile::new("exact.kgr", text),
            ParseOptions {
                limits: ParseLimits {
                    max_diagnostics: limit,
                    ..Default::default()
                },
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert_eq!(parsed.diagnostics().len(), limit);
        assert!(
            parsed
                .diagnostics()
                .iter()
                .all(|d| !matches!(d.kind, DiagnosticKind::CompileLimitExceeded { .. }))
        );
    }
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(
        parse(
            &SourceFile::new("cancel.kgr", "@"),
            ParseOptions {
                limits: ParseLimits {
                    max_diagnostics: 0,
                    ..Default::default()
                },
                ..Default::default()
            },
            &cancel
        )
        .is_err()
    );
}

#[test]
fn recursive_grammar_families_stop_at_nesting_budget() {
    let depth = 2_000;
    let cases = [
        format!(
            "fn main() {{ {}1{} }}",
            "(".repeat(depth),
            ")".repeat(depth)
        ),
        format!("fn main() {{ {}true }}", "!".repeat(depth)),
        format!(
            "fn main() {{ {}1{} }}",
            "[".repeat(depth),
            "]".repeat(depth)
        ),
        format!(
            "fn main(x: {}i32{}) {{}}",
            "[".repeat(depth),
            "]".repeat(depth)
        ),
        format!(
            "fn main(x: {}i32{}) {{}}",
            "Box<".repeat(depth),
            ">".repeat(depth)
        ),
        format!(
            "{} fn inner() {{}} {}",
            "mod nested {".repeat(depth),
            "}".repeat(depth)
        ),
        format!(
            "use {}item{};",
            "nested::{".repeat(depth),
            "}".repeat(depth)
        ),
        format!(
            "fn main() {{ {}1{} }}",
            "if true {".repeat(depth),
            "}".repeat(depth)
        ),
        format!("fn main() {{ {} 1 }}", "if true {} else ".repeat(depth)),
        format!(
            "fn main() {{ match 1 {{ {}_{} => 1 }} }}",
            "(".repeat(depth),
            ",)".repeat(depth)
        ),
        format!(
            "fn main() {{ val {}_{} = value; }}",
            "(".repeat(depth),
            ",)".repeat(depth)
        ),
        format!(
            "fn main() {{ val {}_{} = value; }}",
            "Point { x: ".repeat(depth),
            ", .. }".repeat(depth)
        ),
    ];
    for (index, text) in cases.into_iter().enumerate() {
        let source = SourceFile::new("nesting.kgr", text);
        for max_nesting in [8, 64] {
            let parsed = parse(
                &source,
                ParseOptions {
                    limits: ParseLimits {
                        max_nesting,
                        max_diagnostics: 0,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                &Default::default(),
            )
            .unwrap();
            assert_eq!(parsed.diagnostics().len(), 1, "case {index}");
            assert_eq!(
                parsed.diagnostics()[0].kind,
                DiagnosticKind::CompileLimitExceeded {
                    resource: "parser nesting",
                    limit: max_nesting
                },
                "case {index}"
            );
            assert_eq!(parsed.module().text().to_owned(), source.text());
        }
    }
}

#[test]
fn nesting_budget_is_released_between_sibling_expressions() {
    let source = SourceFile::new(
        "siblings.kgr",
        format!("fn main() {{ {} }}", "1;".repeat(2_000)),
    );
    let limits = ParseLimits {
        max_nesting: 3,
        ..Default::default()
    };
    assert!(
        parse(
            &source,
            ParseOptions {
                limits,
                ..Default::default()
            },
            &Default::default()
        )
        .unwrap()
        .diagnostics()
        .is_empty()
    );
    let parsed = parse(
        &source,
        ParseOptions {
            limits: ParseLimits {
                max_nesting: 2,
                ..limits
            },
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert!(matches!(
        parsed.diagnostics()[0].kind,
        DiagnosticKind::CompileLimitExceeded {
            resource: "parser nesting",
            limit: 2
        }
    ));
}

#[test]
fn iterative_expression_chains_obey_tree_depth_budget() {
    for suffix in [" + 1", ".field", "()", "[0]", "().field[0] + 1"] {
        let source = SourceFile::new(
            "chains.kgr",
            format!(
                "fn main() {{ value{} }} // 中文 😀\r\n",
                suffix.repeat(2_000)
            ),
        );
        let parsed = parse(
            &source,
            ParseOptions {
                limits: ParseLimits {
                    max_tree_depth: 24,
                    ..Default::default()
                },
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert_eq!(parsed.diagnostics().len(), 1, "{suffix}");
        assert_eq!(
            parsed.diagnostics()[0].kind,
            DiagnosticKind::CompileLimitExceeded {
                resource: "syntax tree depth",
                limit: 24
            }
        );
        assert_eq!(parsed.module().text().to_owned(), source.text());
        let nodes = parsed.module().descendants().count();
        assert!(nodes < 200, "chain kept growing: {suffix}, {nodes}");
    }
}

#[test]
fn tree_depth_accounts_for_wrappers_and_not_sibling_width() {
    for text in [
        "",
        "fn main() { 1 + 2 * 3; value.field()[0] }",
        "fn main() { (1, 2, 3) }",
        "fn main() { val x = 1; x = 2; }",
    ] {
        let source = SourceFile::new("depth.kgr", text);
        let baseline = parse(&source, ParseOptions::default(), &Default::default())
            .expect("fresh cancellation token");
        assert!(baseline.diagnostics().is_empty());
        let tree = baseline.module();
        fn depth(node: NodeRef<'_>) -> usize {
            1 + node.children().map(depth).max().unwrap_or(0)
        }
        let depth = depth(NodeRef::ModuleBody(tree.body()));
        let exact = parse(
            &source,
            ParseOptions {
                limits: ParseLimits {
                    max_tree_depth: depth,
                    ..Default::default()
                },
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert!(exact.diagnostics().is_empty(), "{text}");
        let short = parse(
            &source,
            ParseOptions {
                limits: ParseLimits {
                    max_tree_depth: depth - 1,
                    ..Default::default()
                },
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert!(
            matches!(
                short.diagnostics()[0].kind,
                DiagnosticKind::CompileLimitExceeded {
                    resource: "syntax tree depth",
                    ..
                }
            ),
            "{text}"
        );
    }
    let source = SourceFile::new(
        "wide.kgr",
        format!("fn main() {{ [{}] }}", "1,".repeat(2_000)),
    );
    let parsed = parse(
        &source,
        ParseOptions {
            limits: ParseLimits {
                max_tree_depth: 8,
                ..Default::default()
            },
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert!(parsed.diagnostics().is_empty());
}
