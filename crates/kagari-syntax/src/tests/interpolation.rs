use crate::{
    ast::traits::AstNode,
    kind::SyntaxKind,
    lexer::lex,
    parser::{ParseLimits, parse, parse_with_limits},
    token::TokenKind,
};

use {
    kagari_common::cancellation::CancellationToken,
    kagari_source::{diagnostic::DiagnosticKind, source::SourceFile},
};

#[test]
fn interpolation_is_lossless_with_names_multiline_text_and_escapes() {
    let text = r##"fn main() {
        "你好 \u{1f600} {{}} \u{7B}literal\u{7D}
  {value} {other:?} {value}";
    }"##;
    let source = SourceFile::new("interpolation.kgr", text.replace('\n', "\r\n"));
    let parsed = parse(&source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    let syntax = parsed.syntax();
    assert_eq!(
        syntax
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::InterpolatedString)
            .count(),
        1
    );
    assert_eq!(
        syntax
            .syntax()
            .descendants()
            .filter(|n| n.kind() == SyntaxKind::Interpolation)
            .count(),
        3
    );
    let tokens = lex(source.text());
    let value = tokens
        .iter()
        .find(|token| {
            token.kind == TokenKind::Ident
                && token.span.start == source.text().find("{value}").unwrap() + 1
        })
        .unwrap();
    assert_eq!(&source.text()[value.span.start..value.span.end], "value");
}

#[test]
fn interpolation_rejects_expressions_whitespace_keywords_and_old_prefixes() {
    for hole in [
        "",
        " value",
        "value ",
        "value :?",
        "value: ?",
        "value:? ",
        "value\n",
        "value\r\n",
        "value.field",
        "values[0]",
        "get()",
        "module::VALUE",
        "value+1",
        "if true { value }",
        "/*x*/value",
        "true",
        "if",
        "1",
        "value:04",
        "value:",
        "value:?x",
        "Point { .. }",
    ] {
        let source = SourceFile::new(
            "invalid.kgr",
            format!("fn main() {{ \"{{{hole}}}\"; }} fn next() {{}}"),
        );
        let parsed = parse(&source);
        assert!(parsed.diagnostics().iter().any(|diagnostic| matches!(&diagnostic.kind, DiagnosticKind::InvalidLiteral { reason } if reason.contains("bind expressions outside"))), "{hole:?}: {:?}", parsed.diagnostics());
        assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
        assert_eq!(parsed.syntax().items().count(), 2, "{hole:?}");
    }
    for literal in [r#"f"old""#, r#"f"{name}""#, r#"fr"raw""#, r#"rb"raw""#] {
        let source = SourceFile::new("prefix.kgr", format!("fn main() {{ {literal}; }}"));
        assert!(!parse(&source).diagnostics().is_empty(), "{literal}");
    }
}

#[test]
fn incomplete_interpolation_and_invalid_text_report_diagnostics() {
    for literal in [
        r#""{""#,
        r#""}""#,
        r#""{name""#,
        r#""unterminated"#,
        r#""\q""#,
        r#""\u{zz}""#,
        r#""\{""#,
    ] {
        let source = SourceFile::new("invalid.kgr", format!("fn main() {{ {literal}; }}"));
        let parsed = parse(&source);
        assert!(!parsed.diagnostics().is_empty(), "{literal}");
        assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    }
}

#[test]
fn string_literal_only_contexts_admit_raw_and_no_hole_cooked_text() {
    for literal in [
        r#""{{name}}""#,
        r#""\u{7B}name\u{7D}""#,
        r##"r#"{name}"#"##,
        "\"a\r\nb\"",
    ] {
        let source = SourceFile::new(
            "literal.kgr",
            format!(
                "#[meta(name = {literal})] const TEXT: String = {literal}; fn main(s: String) {{ match s {{ {literal} => 1, _ => 0 }} }}"
            ),
        );
        let parsed = parse(&source);
        assert!(
            parsed.diagnostics().is_empty(),
            "{literal}: {:?}",
            parsed.diagnostics()
        );
        assert!(
            !lex(literal)
                .iter()
                .any(|t| t.kind == TokenKind::FormatStart)
        );
    }
    for text in [
        r#"#[meta(name = "{name}")] fn main() {}"#,
        r#"fn main(s: String) { match s { "{name}" => 1, _ => 0 } }"#,
    ] {
        assert!(
            !parse(&SourceFile::new("hole.kgr", text))
                .diagnostics()
                .is_empty(),
            "{text}"
        );
    }
}

#[test]
fn interpolation_recovery_is_bounded_and_cancellable() {
    let source = SourceFile::new(
        "many.kgr",
        format!("fn main() {{ \"{}\" }}", "{x + y}".repeat(2000)),
    );
    let parsed = parse_with_limits(
        &source,
        ParseLimits {
            max_diagnostics: 16,
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
