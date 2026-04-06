use crate::{lexer::lex, tests::common, token::TokenKind};

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
    let source = common::source("struct Player { hp: i32 }");
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
fn lexes_let_and_return_keywords() {
    let source = common::source("let value = 1; return value;");
    let tokens = lex(source.text());
    let kinds: Vec<_> = tokens.into_iter().map(|token| token.kind).collect();

    assert_eq!(
        kinds,
        vec![
            TokenKind::LetKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Whitespace,
            TokenKind::Eq,
            TokenKind::Whitespace,
            TokenKind::Number,
            TokenKind::Semi,
            TokenKind::Whitespace,
            TokenKind::ReturnKw,
            TokenKind::Whitespace,
            TokenKind::Ident,
            TokenKind::Semi,
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
