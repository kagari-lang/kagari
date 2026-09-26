use kagari_common::Span;
use kagari_common::cancellation::{CancellationToken, Cancelled};
use smallvec::SmallVec;

use crate::TokenBuffer;
use crate::token::{Token, TokenKind};

pub fn lex(input: &str) -> TokenBuffer {
    lex_with_cancellation(input, &CancellationToken::default()).expect("fresh cancellation token")
}

pub fn lex_with_cancellation(
    input: &str,
    cancel: &CancellationToken,
) -> Result<TokenBuffer, Cancelled> {
    cancel.check()?;
    let mut chars = input
        .char_indices()
        .take_while(|_| cancel.check().is_ok())
        .peekable();
    let mut tokens = SmallVec::new();

    while let Some((index, ch)) = chars.peek().copied() {
        if ch.is_whitespace() {
            let mut end = index;
            while let Some((next_index, next)) = chars.peek().copied() {
                if !next.is_whitespace() {
                    break;
                }
                end = next_index + next.len_utf8();
                chars.next();
            }
            tokens.push(token(TokenKind::Whitespace, index, end));
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
                if let Some((end, ':')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::ColonColon, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Colon, index, index + 1));
                }
            }
            ';' => {
                chars.next();
                tokens.push(token(TokenKind::Semi, index, index + 1));
            }
            '@' => {
                chars.next();
                tokens.push(token(TokenKind::At, index, index + 1));
            }
            '.' => {
                chars.next();
                if let Some((dot, '.')) = chars.peek().copied() {
                    chars.next();
                    if let Some((equal, '=')) = chars.peek().copied() {
                        chars.next();
                        tokens.push(token(TokenKind::DotDotEq, index, equal + 1));
                    } else {
                        tokens.push(token(TokenKind::DotDot, index, dot + 1));
                    }
                } else {
                    tokens.push(token(TokenKind::Dot, index, index + 1));
                }
            }
            '+' => {
                chars.next();
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::PlusEq, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Plus, index, index + 1));
                }
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
                    if let Some((end, '=')) = chars.peek().copied() {
                        chars.next();
                        tokens.push(token(TokenKind::MinusEq, index, end + 1));
                    } else {
                        tokens.push(token(TokenKind::Minus, index, index + 1));
                    }
                }
            }
            '*' => {
                chars.next();
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::StarEq, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Star, index, index + 1));
                }
            }
            '/' => {
                chars.next();
                if chars.peek().is_some_and(|(_, ch)| *ch == '/') {
                    let mut end = index + 1;
                    while let Some((next_index, next)) = chars.peek().copied() {
                        if matches!(next, '\r' | '\n') {
                            break;
                        }
                        end = next_index + next.len_utf8();
                        chars.next();
                    }
                    tokens.push(token(TokenKind::LineComment, index, end));
                    continue;
                }
                if chars.peek().is_some_and(|(_, ch)| *ch == '*') {
                    let (star, _) = chars.next().expect("block comment opener");
                    let mut end = star + 1;
                    let mut depth = 1usize;
                    while let Some((next_index, next)) = chars.next() {
                        end = next_index + next.len_utf8();
                        if next == '/' && chars.peek().is_some_and(|(_, ch)| *ch == '*') {
                            let (star, _) = chars.next().expect("nested comment opener");
                            end = star + 1;
                            depth += 1;
                        } else if next == '*' && chars.peek().is_some_and(|(_, ch)| *ch == '/') {
                            let (slash, _) = chars.next().expect("block comment closer");
                            end = slash + 1;
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                    }
                    let kind = if depth == 0 {
                        TokenKind::BlockComment
                    } else {
                        TokenKind::Unknown
                    };
                    tokens.push(token(kind, index, end));
                    continue;
                }
                if let Some((end, '=')) = chars.peek().copied() {
                    chars.next();
                    tokens.push(token(TokenKind::SlashEq, index, end + 1));
                } else {
                    tokens.push(token(TokenKind::Slash, index, index + 1));
                }
            }
            '%' => {
                chars.next();
                tokens.push(token(TokenKind::Percent, index, index + 1));
            }
            '?' => {
                tokens.push(token(TokenKind::Question, index, index + 1));
                chars.next();
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
                    tokens.push(token(TokenKind::Pipe, index, index + 1));
                }
            }
            '"' => {
                chars.next();
                let mut end = index + 1;
                let mut escaped = false;
                let mut closed = false;
                while let Some((next_index, next)) = chars.peek().copied() {
                    if matches!(next, '\r' | '\n') {
                        break;
                    }
                    chars.next();
                    end = next_index + next.len_utf8();
                    if escaped {
                        escaped = false;
                        continue;
                    }
                    if next == '\\' {
                        escaped = true;
                        continue;
                    }
                    if next == '"' {
                        closed = true;
                        break;
                    }
                }
                let kind = if closed
                    && kagari_common::literal::decode_string_literal(&input[index..end]).is_ok()
                {
                    TokenKind::String
                } else {
                    TokenKind::Unknown
                };
                tokens.push(token(kind, index, end));
            }
            '0'..='9' => {
                chars.next();
                let mut end = index + 1;
                if ch == '0'
                    && chars
                        .peek()
                        .is_some_and(|(_, ch)| matches!(ch, 'b' | 'o' | 'x'))
                {
                    let (prefix, _) = chars.next().expect("integer base prefix");
                    end = prefix + 1;
                    while let Some((next_index, next)) = chars.peek().copied() {
                        if !(next.is_ascii_alphanumeric() || next == '_') {
                            break;
                        }
                        chars.next();
                        end = next_index + 1;
                    }
                    let kind = if kagari_common::literal::is_integer_literal(&input[index..end]) {
                        TokenKind::Number
                    } else {
                        TokenKind::Unknown
                    };
                    tokens.push(token(kind, index, end));
                    continue;
                }
                while let Some((next_index, next)) = chars.peek().copied() {
                    if !(next.is_ascii_digit() || next == '_') {
                        break;
                    }
                    chars.next();
                    end = next_index + 1;
                }
                let mut kind = TokenKind::Number;
                if chars.peek().is_some_and(|(dot, next)| {
                    *next == '.'
                        && input
                            .as_bytes()
                            .get(dot + 1)
                            .is_some_and(u8::is_ascii_digit)
                }) {
                    let (dot, _) = chars.next().expect("fraction dot");
                    end = dot + 1;
                    kind = TokenKind::Float;
                    while let Some((next_index, next)) = chars.peek().copied() {
                        if !(next.is_ascii_digit() || next == '_') {
                            break;
                        }
                        chars.next();
                        end = next_index + 1;
                    }
                }
                if chars.peek().is_some_and(|(_, ch)| matches!(ch, 'e' | 'E')) {
                    let (exponent, _) = chars.next().expect("exponent marker");
                    end = exponent + 1;
                    kind = TokenKind::Float;
                    if chars.peek().is_some_and(|(_, ch)| matches!(ch, '+' | '-')) {
                        let (sign, _) = chars.next().expect("exponent sign");
                        end = sign + 1;
                    }
                    if !chars.peek().is_some_and(|(_, ch)| ch.is_ascii_digit()) {
                        kind = TokenKind::Unknown;
                    }
                    while let Some((next_index, next)) = chars.peek().copied() {
                        if !(next.is_ascii_digit() || next == '_') {
                            break;
                        }
                        chars.next();
                        end = next_index + 1;
                    }
                }
                if kind == TokenKind::Number
                    && !kagari_common::literal::is_integer_literal(&input[index..end])
                {
                    kind = TokenKind::Unknown;
                }
                tokens.push(token(kind, index, end));
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
                    "as" => TokenKind::AsKw,
                    "crate" => TokenKind::CrateKw,
                    "for" => TokenKind::ForKw,
                    "in" => TokenKind::InKw,
                    "fn" => TokenKind::FnKw,
                    "impl" => TokenKind::ImplKw,
                    "mod" => TokenKind::ModKw,
                    "pub" => TokenKind::PubKw,
                    "self" => TokenKind::SelfKw,
                    "super" => TokenKind::SuperKw,
                    "trait" => TokenKind::TraitKw,
                    "type" => TokenKind::TypeKw,
                    "use" => TokenKind::UseKw,
                    "where" => TokenKind::WhereKw,
                    "const" => TokenKind::ConstKw,
                    "val" => TokenKind::ValKw,
                    "var" => TokenKind::VarKw,
                    "struct" => TokenKind::StructKw,
                    "enum" => TokenKind::EnumKw,
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
                tokens.push(token(TokenKind::Unknown, index, index + ch.len_utf8()));
            }
        }
    }

    let eof = input.len();
    tokens.push(token(TokenKind::Eof, eof, eof));
    cancel.check()?;
    Ok(tokens)
}

fn token(kind: TokenKind, start: usize, end: usize) -> Token {
    Token {
        kind,
        span: Span::new(start, end),
    }
}
