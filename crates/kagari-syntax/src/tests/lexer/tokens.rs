use crate::{ast::traits::AstNode, lexer::lex, parser::parse, tests::common, token::TokenKind};

#[test]
fn character_tokens_keep_unicode_spans_and_pattern_boundaries() {
    let source = r"'🙂' 'a'..='z' '\'' '\u{10_FFFF_}'";
    let tokens = lex(source)
        .into_iter()
        .filter(|token| !token.kind.is_trivia())
        .collect::<Vec<_>>();
    assert_eq!(
        tokens
            .iter()
            .map(|token| token.kind.clone())
            .collect::<Vec<_>>(),
        [
            TokenKind::Char,
            TokenKind::Char,
            TokenKind::DotDotEq,
            TokenKind::Char,
            TokenKind::Char,
            TokenKind::Char,
            TokenKind::Eof,
        ]
    );
    assert_eq!(&source[tokens[0].span.start..tokens[0].span.end], "'🙂'");
    let source = "const FIRST: char = 'a'; fn classify(c: char) -> char { match c { 'a'..='z' => '🙂', _ => '\\0' } }";
    let parsed = common::parse(source);
    assert!(
        parsed.diagnostics().is_empty(),
        "{:?}",
        parsed.diagnostics()
    );
    for malformed in ["''", "'ab'", "'á'", "'\t'", r"'\x80'", r"'\u{D800}'"] {
        assert_eq!(lex(malformed)[0].kind, TokenKind::Unknown, "{malformed:?}");
    }
    let broken = "'unterminated\r\nfn next() {}";
    let tokens = lex(broken);
    assert_eq!(tokens[0].kind, TokenKind::Unknown);
    assert_eq!(
        &broken[tokens[0].span.start..tokens[0].span.end],
        "'unterminated"
    );
    assert!(tokens.iter().any(|token| token.kind == TokenKind::FnKw));
}

#[test]
fn byte_tokens_preserve_source_spans_and_never_interpolate() {
    for (literal, kind) in [
        ("b'A'", TokenKind::ByteChar),
        (r"b'\xFF'", TokenKind::ByteChar),
        (r#"b"{name} {{}} } { \xFF""#, TokenKind::ByteString),
        ("b\"a\r\n  b\"", TokenKind::ByteString),
        ("b\"a\\\r\n \tb\"", TokenKind::ByteString),
        ("br\"a\\\r\n b\"", TokenKind::ByteString),
    ] {
        let tokens = lex(literal);
        assert_eq!(tokens[0].kind, kind, "{literal:?}");
        assert_eq!(&literal[tokens[0].span.start..tokens[0].span.end], literal);
        assert_eq!(tokens.len(), 2);
    }
    for count in [0, 1, 2, 255] {
        let hashes = "#".repeat(count);
        let text = format!("br{hashes}\"{{name}}\\xFF\r\n\"{hashes}# next");
        let tokens = lex(&text);
        assert_eq!(tokens[0].kind, TokenKind::ByteString, "{count}");
        assert_eq!(tokens[0].span.end, text.find("# next").unwrap());
        assert_eq!(tokens[1].kind, TokenKind::Hash);
    }
    for text in [
        "b'é'",
        "b'🙂'",
        "b''",
        "b'ab'",
        r"b'\u{41}'",
        r#"b"\u{41}""#,
        r#"b"中文""#,
        r#"br"é""#,
        "b\"a\rb\"",
        r#"b"\xGG""#,
    ] {
        assert_eq!(lex(text)[0].kind, TokenKind::Unknown, "{text:?}");
    }
    let too_many = format!("br{}\"x\"", "#".repeat(256));
    assert_eq!(lex(&too_many)[0].kind, TokenKind::Unknown);
    for text in [
        "b\"unterminated\r\nfn next() {}",
        "br#\"open\n\" fn next() {}",
    ] {
        let tokens = lex(text);
        assert_eq!(tokens[0].kind, TokenKind::Unknown);
        assert_eq!(tokens[0].span.end, text.len());
        assert_eq!(tokens.len(), 2);
    }
    let text = "b'unterminated\r\nfn next() {}";
    let tokens = lex(text);
    assert_eq!(tokens[0].kind, TokenKind::Unknown);
    assert_eq!(tokens[0].span.end, text.find('\r').unwrap());
    assert!(tokens.iter().any(|token| token.kind == TokenKind::FnKw));
}

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
fn string_tokens_preserve_physical_lines_and_raw_delimiter_boundaries() {
    for literal in [
        "\"a\r\n  🙂\n\"",
        "\"a\\\r\n \tb\"",
        r#""\u{7B}name\u{7D}""#,
        r#""{{name}}""#,
    ] {
        let tokens = lex(literal);
        assert_eq!(tokens[0].kind, TokenKind::String, "{literal:?}");
        assert_eq!(&literal[tokens[0].span.start..tokens[0].span.end], literal);
        assert_eq!(tokens.len(), 2);
    }
    for count in [0, 1, 2, 255] {
        let hashes = "#".repeat(count);
        let text = format!("r{hashes}\"{{name}}\\\r\n🙂\"{hashes}# next");
        let tokens = lex(&text);
        assert_eq!(tokens[0].kind, TokenKind::String, "{count}");
        assert_eq!(tokens[0].span.end, text.find("# next").unwrap());
        assert_eq!(tokens[1].kind, TokenKind::Hash);
    }
    let text = "r##\"a\"#b\"##";
    assert_eq!(lex(text)[0].span.end, text.len());
    for text in ["\"a\rb\"", "r\"a\rb\"", "\"a\\\rb\""] {
        assert_eq!(lex(text)[0].kind, TokenKind::Unknown, "{text:?}");
    }
    let too_many = format!("r{}\"x\"", "#".repeat(256));
    assert_eq!(lex(&too_many)[0].kind, TokenKind::Unknown);
}

#[test]
fn unterminated_multiline_strings_end_at_eof_not_the_next_declaration() {
    for text in [
        "\"unterminated\r\nfn next() {{}}",
        "r#\"unterminated\n\"\nfn next() {}",
    ] {
        let tokens = lex(text);
        assert_eq!(tokens[0].kind, TokenKind::Unknown);
        assert_eq!(tokens[0].span.end, text.len());
        assert_eq!(tokens.len(), 2);
    }

    // Undoubled braces still form hole tokens inside an unclosed cooked string.
    let text = "\"unterminated\r\nfn next() {}";
    let tokens = lex(text);
    assert_eq!(
        tokens.iter().map(|token| &token.kind).collect::<Vec<_>>(),
        [
            &TokenKind::FormatStart,
            &TokenKind::FormatText,
            &TokenKind::FormatOpen,
            &TokenKind::FormatClose,
            &TokenKind::Eof,
        ]
    );
    assert_eq!(tokens[0].span.start, 0);
    assert_eq!(tokens.last().unwrap().span.end, text.len());
    for pair in tokens.windows(2) {
        assert_eq!(pair[0].span.end, pair[1].span.start);
    }
    let parsed = common::parse(text);
    assert!(!parsed.diagnostics().is_empty());
    assert_eq!(parsed.syntax().syntax().text().to_string(), text);
}

#[test]
fn line_comments_keep_unicode_ranges_and_crlf_trivia() {
    let text = "a /= 2; // 中文😀\r\n// end";
    let tokens = lex(text);
    let comments: Vec<_> = tokens
        .iter()
        .filter(|token| token.kind == TokenKind::LineComment)
        .map(|token| &text[token.span.start..token.span.end])
        .collect();
    assert_eq!(comments, ["// 中文😀", "// end"]);
    assert!(tokens.iter().any(|token| token.kind == TokenKind::SlashEq));
    assert!(
        tokens
            .iter()
            .any(|token| token.kind == TokenKind::Whitespace
                && &text[token.span.start..token.span.end] == "\r\n")
    );
}

#[test]
fn nested_block_comments_are_lossless_trivia_and_unclosed_comments_reject() {
    let source = "fn main() { /* 中😀\r\n /* nested */ done */ 42 }";
    let tokens = lex(source);
    let comment = tokens
        .iter()
        .find(|token| token.kind == TokenKind::BlockComment)
        .expect("block comment token");
    assert_eq!(
        &source[comment.span.start..comment.span.end],
        "/* 中😀\r\n /* nested */ done */"
    );
    assert!(common::parse(source).diagnostics().is_empty());
    assert!(!common::parse("/* never closed").diagnostics().is_empty());
}

#[test]
fn numeric_and_escaped_string_tokens_keep_range_boundaries() {
    let source = r#"0b1010 0o12 0x2a 1_000 1.25e+2 1e2 1..2 1..=2 "a\"b\n\u{4E2D}""#;
    let tokens = lex(source)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Whitespace)
        .map(|token| token.kind)
        .collect::<Vec<_>>();
    assert_eq!(
        tokens,
        [
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::Float,
            TokenKind::Float,
            TokenKind::Number,
            TokenKind::DotDot,
            TokenKind::Number,
            TokenKind::Number,
            TokenKind::DotDotEq,
            TokenKind::Number,
            TokenKind::String,
            TokenKind::Eof,
        ]
    );
    for malformed in [
        "0b",
        "0xgg",
        "1e+",
        "1isize",
        "1usize",
        r#""\q""#,
        r#""\u{110000}""#,
    ] {
        assert_eq!(lex(malformed)[0].kind, TokenKind::Unknown, "{malformed}");
    }
}

#[test]
fn unsupported_unicode_tokens_keep_utf8_boundaries_and_parse_without_panicking() {
    let text = "fn main() { 中😀 }";
    let unknown = lex(text)
        .into_iter()
        .filter(|token| token.kind == TokenKind::Unknown)
        .map(|token| text[token.span.start..token.span.end].to_owned())
        .collect::<Vec<_>>();
    assert_eq!(unknown, ["中", "😀"]);
    assert!(!parse(&common::source(text)).diagnostics().is_empty());
}

#[test]
fn lexes_function_signature_tokens() {
    let source = common::source("fn add(lhs: int) -> int {}");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::FnKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::RParen,
            TokenKind::Whitespace,
            TokenKind::Arrow,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_struct_definition_tokens() {
    let source = common::source("struct Player { val hp: i32 }");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::StructKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::Whitespace,
            TokenKind::ValKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_enum_and_match_tokens() {
    let source = common::source("enum Color { Red } match color { Red => 1, _ => 2 }");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::EnumKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::RBrace,
            TokenKind::Whitespace,
            TokenKind::MatchKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::FatArrow,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Comma,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::FatArrow,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Whitespace,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_array_and_tuple_tokens() {
    let source = common::source("[1, 2] (value, true) [i32]");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::LBracket,
            TokenKind::Number,
            TokenKind::Comma,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::RBracket,
            TokenKind::Whitespace,
            TokenKind::LParen,
            TokenKind::Ident,
            TokenKind::Comma,
            TokenKind::Whitespace,
            TokenKind::TrueKw,
            TokenKind::RParen,
            TokenKind::Whitespace,
            TokenKind::LBracket,
            TokenKind::Ident,
            TokenKind::RBracket,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_static_keyword_and_unreserved_words() {
    let source = common::source("let static mut ref dyn static_value");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::StaticKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_val_and_var_keywords() {
    let source = common::source("val answer = 42; var score = answer;");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::ValKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Eq,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Semi,
            TokenKind::Whitespace,
            TokenKind::VarKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Eq,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Semi,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_module_loading_tokens() {
    let source =
        common::source("pub use crate::foo::{bar as baz}; mod gameplay; self::value super::x");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::PubKw,
            TokenKind::Whitespace,
            TokenKind::UseKw,
            TokenKind::Whitespace,
            TokenKind::CrateKw,
            TokenKind::ColonColon,
            TokenKind::Ident,
            TokenKind::ColonColon,
            TokenKind::LBrace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::AsKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::RBrace,
            TokenKind::Semi,
            TokenKind::Whitespace,
            TokenKind::ModKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Semi,
            TokenKind::Whitespace,
            TokenKind::SelfKw,
            TokenKind::ColonColon,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::SuperKw,
            TokenKind::ColonColon,
            TokenKind::Ident,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_expression_literals_and_operators() {
    let source = common::source("true && false || 1.5 + \"ok\" != 0");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::TrueKw,
            TokenKind::Whitespace,
            TokenKind::AmpAmp,
            TokenKind::Whitespace,
            TokenKind::FalseKw,
            TokenKind::Whitespace,
            TokenKind::PipePipe,
            TokenKind::Whitespace,
            TokenKind::Float,
            TokenKind::Whitespace,
            TokenKind::Plus,
            TokenKind::Whitespace,
            TokenKind::String,
            TokenKind::Whitespace,
            TokenKind::NotEq,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_postfix_tokens() {
    let source = common::source("foo(1).bar[0]");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::Ident,
            TokenKind::LParen,
            TokenKind::Number,
            TokenKind::RParen,
            TokenKind::Dot,
            TokenKind::Ident,
            TokenKind::LBracket,
            TokenKind::Number,
            TokenKind::RBracket,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_control_flow_keywords() {
    let source = common::source("if else while loop break continue");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::IfKw,
            TokenKind::Whitespace,
            TokenKind::ElseKw,
            TokenKind::Whitespace,
            TokenKind::WhileKw,
            TokenKind::Whitespace,
            TokenKind::LoopKw,
            TokenKind::Whitespace,
            TokenKind::BreakKw,
            TokenKind::Whitespace,
            TokenKind::ContinueKw,
            TokenKind::Eof,
        ]
    );
}

#[test]
fn lexes_module_item_keywords() {
    let source = common::source("pub const A = 1; trait T {} impl T for U where U: T {}");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::PubKw,
            TokenKind::Whitespace,
            TokenKind::ConstKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Eq,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Semi,
            TokenKind::Whitespace,
            TokenKind::TraitKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Whitespace,
            TokenKind::ImplKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::ForKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::WhereKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Colon,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::LBrace,
            TokenKind::RBrace,
            TokenKind::Eof,
        ]
    );
}
