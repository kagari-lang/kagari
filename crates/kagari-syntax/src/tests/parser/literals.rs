use crate::{ast::traits::AstNode, tests::common};

#[test]
fn byte_literal_parser_contexts_distinguish_scalars_from_arrays() {
    let source = r##"
        const FIRST: u8 = b'A';
        fn classify(value: u8) -> u8 {
            val bytes = br#"{name}\xFF"#;
            match value { b'A'..=b'Z' => b'\xFF', _ => b'\0' }
        }
    "##;
    let parsed = common::parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source);
    for source in [
        r#"fn bad(value: [u8]) { match value { b"A" => (), _ => () } }"#,
        r#"fn bad(value: [u8]) { match value { br"" => (), _ => () } }"#,
    ] {
        assert!(!common::parse(source).diagnostics().is_empty(), "{source}");
    }
}

#[test]
fn character_literals_parse_in_constants_and_range_patterns() {
    let source = "const FIRST: char = 'a'; fn classify(c: char) -> char { match c { 'a'..='z' => '🙂', _ => '\\0' } }";
    let parsed = common::parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
}

#[test]
fn unterminated_interpolation_retains_lossless_recovery() {
    let text = "\"unterminated\r\nfn next() {}";
    let parsed = common::parse(text);
    assert!(!parsed.diagnostics().is_empty());
    assert_eq!(parsed.syntax().syntax().text().to_string(), text);
}

#[test]
fn nested_comments_parse_and_unclosed_comments_reject() {
    let source = "fn main() { /* 中😀\r\n /* nested */ done */ 42 }";
    assert!(common::parse(source).diagnostics().is_empty());
    assert!(!common::parse("/* never closed").diagnostics().is_empty());
}

#[test]
fn unsupported_unicode_tokens_parse_without_panicking() {
    let text = "fn main() { 中😀 }";
    assert!(!common::parse(text).diagnostics().is_empty());
}
