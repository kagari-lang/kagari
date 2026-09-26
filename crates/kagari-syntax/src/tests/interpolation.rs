use crate::{
    ast::AstNode,
    kind::SyntaxKind,
    parser::{ParseLimits, parse_with_limits},
};
use kagari_common::{DiagnosticKind, SourceFile, cancellation::CancellationToken};

#[test]
fn interpolation_is_lossless_with_nested_expressions_comments_and_escapes() {
    let text = r##"fn main() {
        f"你好 \u{1f600} {{}} {f"{if true { "a}" } else { "b" }}"} { /* } */ 7:?}";
    }"##;
    let source = SourceFile::new("interpolation.kgr", text.replace('\n', "\r\n"));
    let parsed = crate::parse(&source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    assert_eq!(
        parsed
            .syntax()
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::InterpolatedString)
            .count(),
        2
    );
    assert_eq!(
        parsed
            .syntax()
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::Interpolation)
            .count(),
        3
    );
}

#[test]
fn incomplete_interpolation_and_unsupported_formats_are_diagnostics() {
    for literal in [
        r#"f"{""#,
        r#"f"}""#,
        r#"f"{}""#,
        r#"f"{1:04}""#,
        r#"f"{1:}""#,
        r#"f"{1""#,
        r#"f"unterminated"#,
        r#"f"\q""#,
        r#"f"\u{zz}""#,
    ] {
        let source = SourceFile::new("invalid.kgr", format!("fn main() {{ {literal}; }}"));
        let parsed = crate::parse(&source);
        assert!(!parsed.diagnostics().is_empty(), "{literal}");
        assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    }
}

#[test]
fn interpolation_nesting_is_bounded_and_cancellable() {
    let source = SourceFile::new(
        "deep.kgr",
        format!(
            "fn main() {{ {}1{} }}",
            "f\"{".repeat(2000),
            "}\"".repeat(2000)
        ),
    );
    let parsed = parse_with_limits(
        &source,
        ParseLimits {
            max_nesting: 16,
            ..Default::default()
        },
        &CancellationToken::default(),
    )
    .unwrap();
    assert!(
        parsed
            .diagnostics()
            .iter()
            .any(|d| matches!(d.kind, DiagnosticKind::CompileLimitExceeded { .. }))
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    let cancel = CancellationToken::default();
    cancel.cancel();
    assert!(parse_with_limits(&source, Default::default(), &cancel).is_err());
}
