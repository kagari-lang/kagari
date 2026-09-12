use crate::ast::AstNode;
use kagari_common::{SourceFile, cancellation::CancellationToken};

#[test]
fn cancelled_parses_never_return_a_partial_success() {
    let source = SourceFile::new(
        "large",
        format!("fn main() {{\n{}\n}}", " ".repeat(2_000_000)),
    );
    let token = CancellationToken::default();
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| crate::parser::parse_with_cancellation(&source, &token));
        token.cancel();
        assert!(worker.join().unwrap().is_err());
    });
}

#[test]
fn multibyte_whitespace_is_lossless_and_does_not_split_utf8() {
    let source = SourceFile::new("unicode", "fn\u{3000}main()\u{a0}{\u{2003}1\u{3000}}\r\n");
    let parsed = crate::parse(&source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    assert_eq!(parsed.syntax().syntax().text().to_string(), source.text());
    for token in crate::lexer::lex(source.text()) {
        assert!(source.text().is_char_boundary(token.span.start));
        assert!(source.text().is_char_boundary(token.span.end));
    }
}
