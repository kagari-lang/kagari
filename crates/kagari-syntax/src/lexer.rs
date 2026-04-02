use kagari_common::Span;

use crate::token::{Token, TokenKind};

pub fn lex(input: &str) -> Vec<Token> {
    let mut chars = input.char_indices().peekable();
    let mut tokens = Vec::new();

    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        match ch {
            '(' => {
                chars.next();
                tokens.push(token(TokenKind::LParen, index, index + 1));
            }
            ')' => {
                chars.next();
                tokens.push(token(TokenKind::RParen, index, index + 1));
            }
            '{' => {
                chars.next();
                tokens.push(token(TokenKind::LBrace, index, index + 1));
            }
            '}' => {
                chars.next();
                tokens.push(token(TokenKind::RBrace, index, index + 1));
            }
            ',' => {
                chars.next();
                tokens.push(token(TokenKind::Comma, index, index + 1));
            }
            ':' => {
                chars.next();
                tokens.push(token(TokenKind::Colon, index, index + 1));
            }
            ';' => {
                chars.next();
                tokens.push(token(TokenKind::Semi, index, index + 1));
            }
            '-' => {
                chars.next();
                if let Some((end, '>')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::Arrow, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Unknown('-'), index, index + 1));
                }
            }
            '0'..='9' => {
                let mut end = index;
                let mut number = String::new();
                while let Some((next_index, next)) = chars.peek().copied() {
                    if !next.is_ascii_digit() {
                        break;
                    }
                    end = next_index;
                    number.push(next);
                    chars.next();
                }
                tokens.push(token(TokenKind::Number(number), index, end + 1));
            }
            '_' | 'a'..='z' | 'A'..='Z' => {
                let mut end = index;
                let mut ident = String::new();
                while let Some((next_index, next)) = chars.peek().copied() {
                    if !(next == '_' || next.is_ascii_alphanumeric()) {
                        break;
                    }
                    end = next_index;
                    ident.push(next);
                    chars.next();
                }

                let kind = if ident == "fn" {
                    TokenKind::Fn
                } else {
                    TokenKind::Ident(ident)
                };
                tokens.push(token(kind, index, end + 1));
            }
            other => {
                chars.next();
                tokens.push(token(TokenKind::Unknown(other), index, index + 1));
            }
        }
    }

    let eof = input.len();
    tokens.push(token(TokenKind::Eof, eof, eof));
    tokens
}

fn token(kind: TokenKind, start: usize, end: usize) -> Token {
    Token {
        kind,
        span: Span::new(start, end),
    }
}
