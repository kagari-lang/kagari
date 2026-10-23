//! Physical String boundaries and source-only interpolation tokenization.

use std::iter::Peekable;

use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::literal::{self, StringDelimiter};

use crate::{
    TokenBuffer,
    lexer::{identifier_kind, token},
    token::TokenKind,
};

pub(super) fn scan(
    input: &str,
    start: usize,
    chars: &mut Peekable<impl Iterator<Item = (usize, char)>>,
    tokens: &mut TokenBuffer,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    let byte = input[start..].starts_with('b');
    let prefix = usize::from(byte);
    let Ok(Some((delimiter, opening))) = StringDelimiter::opening(&input[start + prefix..]) else {
        // Consume only the invalid raw opener, leaving following source available.
        chars.next();
        if byte {
            chars.next();
        }
        while chars.peek().is_some_and(|(_, ch)| *ch == '#') {
            cancel.check()?;
            chars.next();
        }
        let end = chars.peek().map_or(input.len(), |(index, _)| *index);
        tokens.push(token(TokenKind::Unknown, start, end));
        return Ok(());
    };
    let opening = opening + prefix;
    while chars
        .peek()
        .is_some_and(|(index, _)| *index < start + opening)
    {
        chars.next();
    }
    if byte || matches!(delimiter, StringDelimiter::Raw(_)) {
        return scan_plain(input, start, delimiter, byte, chars, tokens, cancel);
    }

    let first = tokens.len();
    tokens.push(token(TokenKind::FormatStart, start, start + opening));
    let mut text_start = start + opening;
    let mut has_hole = false;
    let mut closed = false;
    let mut end = input.len();
    while let Some((index, ch)) = chars.next() {
        cancel.check()?;
        match ch {
            '"' => {
                push_text(input, text_start, index, tokens, cancel)?;
                end = index + 1;
                tokens.push(token(TokenKind::FormatEnd, index, end));
                closed = true;
                break;
            }
            '\\' => skip_escape(chars, cancel)?,
            '{' | '}' if chars.peek().is_some_and(|(_, next)| *next == ch) => {
                chars.next();
            }
            '{' => {
                has_hole = true;
                push_text(input, text_start, index, tokens, cancel)?;
                tokens.push(token(TokenKind::FormatOpen, index, index + 1));
                scan_hole(input, index + 1, chars, tokens, cancel)?;
                text_start = chars.peek().map_or(input.len(), |(position, _)| *position);
            }
            _ => {}
        }
    }
    if !closed {
        push_text(input, text_start, end, tokens, cancel)?;
    }
    if !has_hole {
        let valid = closed && tokens[first..].iter().all(|t| t.kind != TokenKind::Unknown);
        tokens.truncate(first);
        tokens.push(token(
            if valid {
                TokenKind::String
            } else {
                TokenKind::Unknown
            },
            start,
            end,
        ));
    }
    Ok(())
}

fn scan_plain(
    input: &str,
    start: usize,
    delimiter: StringDelimiter,
    byte: bool,
    chars: &mut Peekable<impl Iterator<Item = (usize, char)>>,
    tokens: &mut TokenBuffer,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    while let Some((index, ch)) = chars.next() {
        cancel.check()?;
        if ch == '\\' && delimiter == StringDelimiter::Cooked {
            skip_escape(chars, cancel)?;
            continue;
        }
        if ch != '"' {
            continue;
        }
        if let Some(length) = delimiter.closing_len(&input[index..]) {
            let end = index + length;
            while chars.peek().is_some_and(|(position, _)| *position < end) {
                chars.next();
            }
            let valid = if byte {
                literal::decode_byte_string_literal_with_cancellation(&input[start..end], cancel)?
                    .is_ok()
            } else {
                literal::decode_string_literal_with_cancellation(&input[start..end], cancel)?
                    .is_ok()
            };
            let kind = match (valid, byte) {
                (true, true) => TokenKind::ByteString,
                (true, false) => TokenKind::String,
                (false, _) => TokenKind::Unknown,
            };
            tokens.push(token(kind, start, end));
            return Ok(());
        }
    }
    tokens.push(token(TokenKind::Unknown, start, input.len()));
    Ok(())
}

// Escape braces cannot become holes. Invalid escapes are still diagnosed by the
// shared decoder; a quote always ends recovery for a malformed Unicode escape.
fn skip_escape(
    chars: &mut Peekable<impl Iterator<Item = (usize, char)>>,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    if chars.next().is_some_and(|(_, ch)| ch == 'u')
        && chars.peek().is_some_and(|(_, ch)| *ch == '{')
    {
        chars.next();
        while let Some((_, ch)) = chars.peek().copied() {
            cancel.check()?;
            if ch == '"' {
                break;
            }
            chars.next();
            if ch == '}' {
                break;
            }
        }
    }
    Ok(())
}

fn push_text(
    input: &str,
    start: usize,
    end: usize,
    tokens: &mut TokenBuffer,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    if start == end {
        return Ok(());
    }
    let kind =
        if literal::decode_string_fragment_with_cancellation(&input[start..end], cancel)?.is_ok() {
            TokenKind::FormatText
        } else {
            TokenKind::Unknown
        };
    tokens.push(token(kind, start, end));
    Ok(())
}

fn scan_hole(
    input: &str,
    start: usize,
    chars: &mut Peekable<impl Iterator<Item = (usize, char)>>,
    tokens: &mut TokenBuffer,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    while let Some((_, ch)) = chars.peek().copied() {
        cancel.check()?;
        if matches!(ch, '}' | '"') {
            break;
        }
        chars.next();
    }
    let end = chars.peek().map_or(input.len(), |(index, _)| *index);
    let content = &input[start..end];
    let name = content.strip_suffix(":?").unwrap_or(content);
    let valid = name.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic())
        && name
            .bytes()
            .all(|ch| cancel.check().is_ok() && (ch == b'_' || ch.is_ascii_alphanumeric()))
        && identifier_kind(name) == TokenKind::Ident;
    cancel.check()?;
    if valid {
        tokens.push(token(TokenKind::Ident, start, start + name.len()));
        if name.len() != content.len() {
            tokens.push(token(TokenKind::Colon, end - 2, end - 1));
            tokens.push(token(TokenKind::Question, end - 1, end));
        }
    } else if start != end {
        tokens.push(token(TokenKind::Unknown, start, end));
    }
    if chars.peek().is_some_and(|(_, ch)| *ch == '}') {
        chars.next();
        tokens.push(token(TokenKind::FormatClose, end, end + 1));
    }
    Ok(())
}
