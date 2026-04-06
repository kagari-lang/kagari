use kagari_common::Span;
use smallvec::SmallVec;

use crate::TokenBuffer;
use crate::token::{Token, TokenKind};

pub fn lex(input: &str) -> TokenBuffer {
    let mut chars = input.char_indices().peekable();
    let mut tokens = SmallVec::new();

    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            let mut end = index;
            while let Some((next_index, next)) = chars.peek().copied() {
                if !next.is_whitespace() {
                    break;
                }
                end = next_index;
                chars.next();
            }
            tokens.push(token(TokenKind::Whitespace, index, end + 1));
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
            '[' => {
                chars.next();
                tokens.push(token(TokenKind::LBracket, index, index + 1));
            }
            ']' => {
                chars.next();
                tokens.push(token(TokenKind::RBracket, index, index + 1));
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
            '.' => {
                chars.next();
                tokens.push(token(TokenKind::Dot, index, index + 1));
            }
            '+' => {
                chars.next();
                tokens.push(token(TokenKind::Plus, index, index + 1));
            }
            '=' => {
                chars.next();
                match chars.peek().copied() {
                    Some((end, '=')) => {
                        chars.next();
                        tokens.push(token(TokenKind::EqEq, index, end + 1));
                    }
                    Some((end, '>')) => {
                        chars.next();
                        tokens.push(token(TokenKind::FatArrow, index, end + 1));
                    }
                    _ => {
                        tokens.push(token(TokenKind::Eq, index, index + 1));
                    }
                }
            }
            '-' => {
                chars.next();
                if let Some((end, '>')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::Arrow, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Minus, index, index + 1));
                }
            }
            '*' => {
                chars.next();
                tokens.push(token(TokenKind::Star, index, index + 1));
            }
            '/' => {
                chars.next();
                tokens.push(token(TokenKind::Slash, index, index + 1));
            }
            '!' => {
                chars.next();
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::NotEq, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Bang, index, index + 1));
                }
            }
            '<' => {
                chars.next();
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::Le, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Lt, index, index + 1));
                }
            }
            '>' => {
                chars.next();
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::Ge, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Gt, index, index + 1));
                }
            }
            '&' => {
                chars.next();
                if let Some((end, '&')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::AmpAmp, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Unknown, index, index + 1));
                }
            }
            '|' => {
                chars.next();
                if let Some((end, '|')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::PipePipe, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Unknown, index, index + 1));
                }
            }
            '"' => {
                chars.next();
                let mut end = index;
                while let Some((next_index, next)) = chars.peek().copied() {
                    end = next_index;
                    chars.next();
                    if next == '"' {
                        break;
                    }
                }
                tokens.push(token(TokenKind::String, index, end + 1));
            }
            '0'..='9' => {
                let mut end = index;
                let mut saw_dot = false;
                while let Some((next_index, next)) = chars.peek().copied() {
                    if next.is_ascii_digit() {
                        end = next_index;
                        chars.next();
                        continue;
                    }
                    if !saw_dot && next == '.' {
                        saw_dot = true;
                        end = next_index;
                        chars.next();
                        continue;
                    }
                    break;
                }
                let kind = if saw_dot {
                    TokenKind::Float
                } else {
                    TokenKind::Number
                };
                tokens.push(token(kind, index, end + 1));
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

                let kind = match ident.as_str() {
                    "fn" => TokenKind::FnKw,
                    "struct" => TokenKind::StructKw,
                    "enum" => TokenKind::EnumKw,
                    "let" => TokenKind::LetKw,
                    "return" => TokenKind::ReturnKw,
                    "if" => TokenKind::IfKw,
                    "else" => TokenKind::ElseKw,
                    "match" => TokenKind::MatchKw,
                    "while" => TokenKind::WhileKw,
                    "loop" => TokenKind::LoopKw,
                    "break" => TokenKind::BreakKw,
                    "continue" => TokenKind::ContinueKw,
                    "true" => TokenKind::TrueKw,
                    "false" => TokenKind::FalseKw,
                    _ => TokenKind::Ident,
                };
                tokens.push(token(kind, index, end + 1));
            }
            _ => {
                chars.next();
                tokens.push(token(TokenKind::Unknown, index, index + 1));
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
