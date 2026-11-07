//! Physical String boundaries and source-only interpolation tokenization.

use std::iter::Peekable;

use kagari_common::cancellation::{CancellationToken, Cancelled};
use kagari_source::literal::{self, StringDelimiter};

use crate::{
    TokenBuffer,
    lexer::{CANCELLATION_CHECK_BYTES, identifier_kind, token},
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
        match ch {
            '"' => {
                push_text(input, text_start, index, tokens, cancel)?;
                end = index + 1;
                tokens.push(token(TokenKind::FormatEnd, index, end));
                closed = true;
                break;
            }
            '\\' => skip_escape(chars),
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
        if ch == '\\' && delimiter == StringDelimiter::Cooked {
            skip_escape(chars);
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

/// Enters just after `\` and consumes the escape discriminator, if present.
/// For `u{`, also consumes through `}` or EOF, leaving a recovery quote unread.
/// Otherwise only the discriminator is consumed, including an escaped quote.
/// This keeps escape braces out of interpolation; the shared decoder diagnoses
/// invalid escapes. All reads use the caller's cancellation-aware iterator.
fn skip_escape(chars: &mut Peekable<impl Iterator<Item = (usize, char)>>) {
    let discriminator = chars.next();
    if matches!(discriminator, Some((_, 'u'))) && chars.peek().is_some_and(|(_, ch)| *ch == '{') {
        chars.next();
        while let Some((_, ch)) = chars.peek().copied() {
            if ch == '"' {
                break;
            }
            chars.next();
            if ch == '}' {
                break;
            }
        }
    }
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

/// Enters just after `{`, with `start` at the first content byte. Consumes a
/// closing `}`, but leaves a quote unread for the enclosing string scanner.
/// A valid name and optional `:?` become separate tokens; nonempty invalid
/// content becomes one `Unknown` token. Empty content emits no name token.
/// Cancellation returns an error rather than classifying the name as invalid.
fn scan_hole(
    input: &str,
    start: usize,
    chars: &mut Peekable<impl Iterator<Item = (usize, char)>>,
    tokens: &mut TokenBuffer,
    cancel: &CancellationToken,
) -> Result<(), Cancelled> {
    while let Some((_, ch)) = chars.peek().copied() {
        if matches!(ch, '}' | '"') {
            break;
        }
        chars.next();
    }
    let end = chars.peek().map_or(input.len(), |(index, _)| *index);
    cancel.check()?;
    let content = &input[start..end];
    let (name, debug) = match content.strip_suffix(":?") {
        Some(name) => (name, true),
        None => (content, false),
    };
    let valid = is_hole_name(name, cancel);
    // Preserve the boundary check even when validation observed cancellation.
    cancel.check()?;
    if valid? {
        tokens.push(token(TokenKind::Ident, start, start + name.len()));
        if debug {
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

/// Checks one untrimmed name, excluding keywords. Polls before validating each
/// byte at the existing chunk boundary; invalid input stops the scan immediately.
fn is_hole_name(name: &str, cancel: &CancellationToken) -> Result<bool, Cancelled> {
    if !name.starts_with(|ch: char| ch == '_' || ch.is_ascii_alphabetic()) {
        return Ok(false);
    }
    for (offset, ch) in name.bytes().enumerate() {
        if offset.is_multiple_of(CANCELLATION_CHECK_BYTES) {
            cancel.check()?;
        }
        if ch != b'_' && !ch.is_ascii_alphanumeric() {
            return Ok(false);
        }
    }
    Ok(identifier_kind(name) == TokenKind::Ident)
}
