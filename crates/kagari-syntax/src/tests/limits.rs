use crate::{
    ast::AstNode,
    parser::{ParseLimits, parse_with_limits},
};
use kagari_common::{DiagnosticKind, SourceFile, cancellation::CancellationToken};

#[test]
fn parser_diagnostic_budget_stops_recovery_and_preserves_lossless_suffix() {
    for prefix in ["", "mod nested {", "trait T {", "impl T {", "fn broken() {"] {
        let text = format!(
            "fn good() -> i32 {{ 42 }} {prefix} {} // 中文 😀\r\nfn later() {{}}",
            "@ ".repeat(500)
        );
        let source = SourceFile::new("limited.kgr", text);
        for max_diagnostics in [0, 1, 7] {
            let parsed = parse_with_limits(
                &source,
                ParseLimits {
                    max_diagnostics,
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
            assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
            assert!(
                !parsed
                    .syntax()
                    .syntax()
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
        let parsed = parse_with_limits(
            &SourceFile::new("exact.kgr", text),
            ParseLimits {
                max_diagnostics: limit,
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
        parse_with_limits(
            &SourceFile::new("cancel.kgr", "@"),
            ParseLimits {
                max_diagnostics: 0,
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
    ];
    for (index, text) in cases.into_iter().enumerate() {
        let source = SourceFile::new("nesting.kgr", text);
        for max_nesting in [8, 64] {
            let parsed = parse_with_limits(
                &source,
                ParseLimits {
                    max_nesting,
                    max_diagnostics: 0,
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
            assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
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
        parse_with_limits(&source, limits, &Default::default())
            .unwrap()
            .diagnostics()
            .is_empty()
    );
    let parsed = parse_with_limits(
        &source,
        ParseLimits {
            max_nesting: 2,
            ..limits
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
