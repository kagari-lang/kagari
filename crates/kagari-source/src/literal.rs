use std::iter::{self, Peekable};

use kagari_common::cancellation::{CancellationToken, Cancelled};

const CANCELLATION_CHECK_BYTES: usize = 4 * 1024;

/// Failure to recognize or decode a source literal, separate from cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LiteralError {
    #[error("invalid integer literal")]
    InvalidIntegerLiteral,
    #[error("floating-point suffix on integer literal")]
    FloatSuffixOnInteger,
    #[error("integer literal is outside the u64 range")]
    IntegerOutOfRange,

    #[error("raw String has more than 255 hashes")]
    TooManyRawStringHashes,
    #[error("invalid raw String opening delimiter")]
    InvalidRawStringOpening,
    #[error("invalid String opening delimiter")]
    InvalidStringOpening,
    #[error("unterminated String literal")]
    UnterminatedString,
    #[error("invalid String delimiter boundary")]
    InvalidStringDelimiterBoundary,
    #[error("unexpected text after raw String delimiter")]
    UnexpectedTextAfterRawStringDelimiter,
    #[error("invalid byte string prefix")]
    InvalidByteStringPrefix,

    #[error("direct byte literal text must be ASCII")]
    NonAsciiByteText,
    #[error("standalone CR in String literal")]
    StandaloneCarriageReturn,
    #[error("incomplete String escape")]
    IncompleteStringEscape,
    #[error("String continuation requires LF or CRLF")]
    InvalidStringContinuation,
    #[error("String text requires doubled braces; interpolation requires a name binding")]
    UndoubledStringBrace,
    #[error("unescaped quote in String text")]
    UnescapedStringQuote,

    #[error("invalid byte character prefix")]
    InvalidByteCharPrefix,
    #[error("unterminated char literal")]
    UnterminatedChar,
    #[error("direct whitespace in char literal")]
    DirectCharWhitespace,
    #[error("unescaped apostrophe in char literal")]
    UnescapedApostrophe,
    #[error("empty char literal")]
    EmptyChar,
    #[error("byte character literal must contain exactly one byte")]
    ByteCharWrongLength,
    #[error("char literal must contain exactly one Unicode scalar")]
    CharWrongLength,

    #[error("invalid ASCII escape")]
    InvalidAsciiEscape,
    #[error("char hexadecimal escape must be ASCII")]
    NonAsciiHexEscape,
    #[error("invalid Unicode scalar")]
    InvalidUnicodeScalar,
    #[error("Unicode escapes are forbidden in byte literals")]
    UnicodeEscapeInByteLiteral,
    #[error("invalid Unicode escape")]
    InvalidUnicodeEscape,
    #[error("invalid char escape")]
    InvalidCharEscape,
}

/// Parse an unsigned integer literal before applying any unary sign or target-type bound.
pub fn parse_integer_literal(text: &str) -> Result<u64, LiteralError> {
    let (radix, digits) = integer_digits(text)?;
    let compact = digits.chars().filter(|ch| *ch != '_').collect::<String>();
    u64::from_str_radix(&compact, radix).map_err(|_| LiteralError::IntegerOutOfRange)
}

/// Separate a primitive type suffix without confusing hexadecimal digits with f32/f64.
pub fn numeric_literal_parts(text: &str) -> (&str, Option<&str>) {
    let type_suffixes = [
        "i64", "u64", "i32", "u32", "i16", "u16", "i8", "u8", "f32", "f64",
    ];
    let non_decimal_prefixes = ["0x", "0o", "0b"];

    for suffix in type_suffixes {
        if suffix.starts_with('f')
            && non_decimal_prefixes
                .iter()
                .any(|prefix| text.starts_with(prefix))
        {
            continue;
        }
        if let Some(digits) = text.strip_suffix(suffix) {
            return (digits, Some(&text[digits.len()..]));
        }
    }
    (text, None)
}

/// Check grammar without imposing a machine-integer size on the lexer.
pub fn is_integer_literal(text: &str) -> bool {
    integer_digits(text).is_ok()
}

fn integer_digits(text: &str) -> Result<(u32, &str), LiteralError> {
    let (text, suffix) = numeric_literal_parts(text);
    if suffix.is_some_and(|suffix| suffix.starts_with('f')) {
        return Err(LiteralError::FloatSuffixOnInteger);
    }
    let (radix, digits) = if let Some(digits) = text.strip_prefix("0b") {
        (2, digits)
    } else if let Some(digits) = text.strip_prefix("0o") {
        (8, digits)
    } else if let Some(digits) = text.strip_prefix("0x") {
        (16, digits)
    } else {
        (10, text)
    };
    if !digits.chars().any(|ch| ch.is_digit(radix)) {
        return Err(LiteralError::InvalidIntegerLiteral);
    }
    if !digits.chars().all(|ch| ch == '_' || ch.is_digit(radix)) {
        return Err(LiteralError::InvalidIntegerLiteral);
    }
    Ok((radix, digits))
}

/// String source delimiter; raw strings have at most 255 hashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringDelimiter {
    /// Ordinary double quotes, with escapes and name interpolation.
    Cooked,
    /// An `r` prefix and the matching number of opening/closing hashes.
    Raw(u8),
}

impl StringDelimiter {
    /// Reads an opening delimiter and its byte length, or returns `None` for non-string source.
    ///
    /// An `r` followed by hashes must reach a quote within the 255-hash limit.
    /// Unsupported prefixes, including `f`, are not String delimiters.
    pub fn opening(text: &str) -> Result<Option<(Self, usize)>, LiteralError> {
        if text.starts_with('"') {
            return Ok(Some((Self::Cooked, 1)));
        }
        let Some(rest) = text.strip_prefix('r') else {
            return Ok(None);
        };
        if !rest.starts_with(['#', '"']) {
            return Ok(None);
        }
        let mut hashes = 0u8;
        for byte in rest.bytes() {
            match byte {
                b'"' => return Ok(Some((Self::Raw(hashes), usize::from(hashes) + 2))),
                b'#' => {
                    hashes = hashes
                        .checked_add(1)
                        .ok_or(LiteralError::TooManyRawStringHashes)?
                }
                _ => break,
            }
        }
        Err(LiteralError::InvalidRawStringOpening)
    }

    /// Returns the closing delimiter length at this position, ignoring subsequent hashes.
    pub fn closing_len(self, text: &str) -> Option<usize> {
        let hashes = match self {
            Self::Cooked => 0,
            Self::Raw(hashes) => usize::from(hashes),
        };
        (text.starts_with('"')
            && text
                .as_bytes()
                .get(1..1 + hashes)
                .is_some_and(|bytes| bytes.iter().all(|b| *b == b'#')))
        .then_some(1 + hashes)
    }
}

/// Decode a complete cooked or raw String without interpolation holes.
///
/// Cooked doubled braces become literal braces; unmatched braces/holes are errors.
/// Raw strings do not interpret braces or escapes. Both normalize physical CRLF
/// only in the returned value and reject standalone CR. Source spans are untouched.
pub fn decode_string_literal(text: &str) -> Result<String, LiteralError> {
    decode_string_literal_with_cancellation(text, &CancellationToken::default())
        .expect("fresh cancellation token")
}

/// Decode a complete String while polling cancellation, including during long escapes.
///
/// The outer error means cancellation; the inner error describes invalid literal syntax.
/// Neither cancellation nor invalid input publishes a partial decoded value.
pub fn decode_string_literal_with_cancellation(
    text: &str,
    cancel: &CancellationToken,
) -> Result<Result<String, LiteralError>, Cancelled> {
    cancel.check()?;
    let mut value = String::new();
    let decoded = decode_string_literal_inner(text, cancel, false, &mut |ch| value.push(ch))?;
    cancel.check()?;
    Ok(decoded.map(|()| value))
}

/// Decode a cooked/raw byte string into bytes, never UTF-8 text.
///
/// Direct text must be ASCII; hexadecimal escapes span `00..FF`. Unicode escapes
/// are forbidden and braces are literal. Delimiters/newlines follow String rules.
pub fn decode_byte_string_literal(text: &str) -> Result<Vec<u8>, LiteralError> {
    decode_byte_string_literal_with_cancellation(text, &CancellationToken::default())
        .expect("fresh cancellation token")
}

/// Decode bytes with the same cancellation/error contract as the String decoder.
pub fn decode_byte_string_literal_with_cancellation(
    text: &str,
    cancel: &CancellationToken,
) -> Result<Result<Vec<u8>, LiteralError>, Cancelled> {
    cancel.check()?;
    let mut value = Vec::new();
    let decoded = match text.strip_prefix('b') {
        Some(text) => {
            decode_string_literal_inner(text, cancel, true, &mut |ch| value.push(ch as u8))?
        }
        None => Err(LiteralError::InvalidByteStringPrefix),
    };
    cancel.check()?;
    Ok(decoded.map(|()| value))
}

/// Separates validated delimiters from their undecoded source content.
fn string_content(text: &str) -> Result<(StringDelimiter, &str), LiteralError> {
    let (delimiter, opening) =
        StringDelimiter::opening(text)?.ok_or(LiteralError::InvalidStringOpening)?;
    let closing = match delimiter {
        StringDelimiter::Cooked => 1,
        StringDelimiter::Raw(hashes) => usize::from(hashes) + 1,
    };
    let end = text
        .len()
        .checked_sub(closing)
        .filter(|end| *end >= opening)
        .ok_or(LiteralError::UnterminatedString)?;
    if text
        .get(end..)
        .and_then(|suffix| delimiter.closing_len(suffix))
        != Some(closing)
    {
        return Err(LiteralError::UnterminatedString);
    }
    let content = text
        .get(opening..end)
        .ok_or(LiteralError::InvalidStringDelimiterBoundary)?;
    Ok((delimiter, content))
}

fn decode_string_literal_inner(
    text: &str,
    cancel: &CancellationToken,
    byte: bool,
    push: &mut impl FnMut(char),
) -> Result<Result<(), LiteralError>, Cancelled> {
    let (delimiter, content) = match string_content(text) {
        Ok(parts) => parts,
        Err(reason) => return Ok(Err(reason)),
    };
    match delimiter {
        StringDelimiter::Cooked => decode_string_content(content, true, byte, cancel, push),
        StringDelimiter::Raw(_) => {
            let mut next_check = CANCELLATION_CHECK_BYTES;
            for (index, ch) in content.char_indices() {
                if index >= next_check {
                    next_check = index.saturating_add(CANCELLATION_CHECK_BYTES);
                    cancel.check()?;
                }
                if ch == '"' && delimiter.closing_len(&content[index..]).is_some() {
                    return Ok(Err(LiteralError::UnexpectedTextAfterRawStringDelimiter));
                }
            }
            decode_string_content(content, false, byte, cancel, push)
        }
    }
}

/// Decode one source text fragment between interpolation holes, without re-quoting.
///
/// Escapes are consumed before source braces: `\u{7B}` produces literal `{`.
/// A fragment cannot contain a hole, lone brace, unescaped quote or invalid escape.
/// Continuations remove the newline and following ASCII space/tab/CR/LF.
pub fn decode_string_fragment(content: &str) -> Result<String, LiteralError> {
    decode_string_fragment_with_cancellation(content, &CancellationToken::default())
        .expect("fresh cancellation token")
}

/// Decode a source fragment with the same cancellation/error contract as the full literal decoder.
pub fn decode_string_fragment_with_cancellation(
    content: &str,
    cancel: &CancellationToken,
) -> Result<Result<String, LiteralError>, Cancelled> {
    cancel.check()?;
    let mut value = String::with_capacity(content.len());
    let decoded = decode_string_content(content, true, false, cancel, &mut |ch| value.push(ch))?;
    cancel.check()?;
    Ok(decoded.map(|()| value))
}

/// Runs decoding with one polling iterator shared by text, escapes and continuations.
/// Cancellation exhausts that iterator internally, but the completion check here
/// distinguishes it from EOF or malformed syntax before returning either result.
/// The callback may receive a prefix; callers must publish it only on `Ok(Ok(()))`.
fn decode_string_content(
    content: &str,
    cooked: bool,
    byte: bool,
    cancel: &CancellationToken,
    push: &mut impl FnMut(char),
) -> Result<Result<(), LiteralError>, Cancelled> {
    let mut next_check = CANCELLATION_CHECK_BYTES;
    let mut chars = content
        .char_indices()
        .take_while(|(offset, _)| {
            if *offset < next_check {
                return true;
            }
            next_check = offset.saturating_add(CANCELLATION_CHECK_BYTES);
            cancel.check().is_ok()
        })
        .map(|(_, ch)| ch)
        .peekable();
    let decoded = decode_string_characters(&mut chars, cooked, byte, push);
    cancel.check()?;
    Ok(decoded)
}

/// Interprets escapes and newlines; the caller owns cancellation and publication.
fn decode_string_characters(
    chars: &mut Peekable<impl Iterator<Item = char>>,
    cooked: bool,
    byte: bool,
    push: &mut impl FnMut(char),
) -> Result<(), LiteralError> {
    while let Some(ch) = chars.next() {
        if byte && !ch.is_ascii() {
            return Err(LiteralError::NonAsciiByteText);
        }
        match ch {
            '\r' => {
                if chars.next() != Some('\n') {
                    return Err(LiteralError::StandaloneCarriageReturn);
                }
                push('\n');
            }
            '\\' if cooked => {
                let next = chars.next().ok_or(LiteralError::IncompleteStringEscape)?;
                if matches!(next, '\n' | '\r') {
                    if next == '\r' && chars.next() != Some('\n') {
                        return Err(LiteralError::InvalidStringContinuation);
                    }
                    while chars
                        .peek()
                        .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
                    {
                        chars.next();
                    }
                } else {
                    push(decode_escape(
                        &mut iter::once(next).chain(chars.by_ref()),
                        byte,
                    )?);
                }
            }
            '{' | '}' if cooked && !byte => {
                if chars.next() != Some(ch) {
                    return Err(LiteralError::UndoubledStringBrace);
                }
                push(ch);
            }
            '"' if cooked => return Err(LiteralError::UnescapedStringQuote),
            _ => push(ch),
        }
    }
    Ok(())
}

/// Decode a single-quoted character literal to exactly one Unicode scalar.
///
/// Escapes follow Rust's character spelling: ASCII `\xNN`, one to six hex
/// digits in `\u{...}` (underscores only after a digit), and simple escapes.
/// Direct tabs/newlines and empty or multiple-scalar contents are errors.
pub fn decode_char_literal(text: &str) -> Result<char, LiteralError> {
    decode_character(text, false)
}

/// Decode a byte character to exactly one byte, with ASCII direct text only.
///
/// Byte escapes include `\x00..\xFF` but never Unicode escapes. Direct tabs and
/// newlines, empty contents and multiple decoded bytes are errors.
pub fn decode_byte_char_literal(text: &str) -> Result<u8, LiteralError> {
    let text = text
        .strip_prefix('b')
        .ok_or(LiteralError::InvalidByteCharPrefix)?;
    decode_character(text, true).map(|ch| ch as u8)
}

fn decode_character(text: &str, byte: bool) -> Result<char, LiteralError> {
    let content = text
        .strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
        .ok_or(LiteralError::UnterminatedChar)?;
    let mut chars = content.chars();
    let value = match chars.next() {
        Some('\\') => decode_escape(&mut chars, byte)?,
        Some('\t' | '\r' | '\n') => return Err(LiteralError::DirectCharWhitespace),
        Some('\'') => return Err(LiteralError::UnescapedApostrophe),
        Some(value) if byte && !value.is_ascii() => return Err(LiteralError::NonAsciiByteText),
        Some(value) => value,
        None => return Err(LiteralError::EmptyChar),
    };
    if chars.next().is_some() {
        return Err(if byte {
            LiteralError::ByteCharWrongLength
        } else {
            LiteralError::CharWrongLength
        });
    }
    Ok(value)
}

fn decode_escape(chars: &mut impl Iterator<Item = char>, byte: bool) -> Result<char, LiteralError> {
    match chars.next() {
        Some('\'') => Ok('\''),
        Some('"') => Ok('"'),
        Some('\\') => Ok('\\'),
        Some('n') => Ok('\n'),
        Some('r') => Ok('\r'),
        Some('t') => Ok('\t'),
        Some('0') => Ok('\0'),
        Some('x') => {
            let mut value = 0;
            for _ in 0..2 {
                let digit = chars
                    .next()
                    .and_then(|ch| ch.to_digit(16))
                    .ok_or(LiteralError::InvalidAsciiEscape)?;
                value = value * 16 + digit;
            }
            if !byte && value > 0x7f {
                return Err(LiteralError::NonAsciiHexEscape);
            }
            char::from_u32(value).ok_or(LiteralError::InvalidUnicodeScalar)
        }
        Some('u') if byte => Err(LiteralError::UnicodeEscapeInByteLiteral),
        Some('u') => {
            if chars.next() != Some('{') {
                return Err(LiteralError::InvalidUnicodeEscape);
            }
            let mut value = 0;
            let mut digits = 0;
            loop {
                match chars.next() {
                    Some('}') if digits > 0 => break,
                    Some('_') if digits > 0 => {}
                    Some(ch) if ch.is_ascii_hexdigit() && digits < 6 => {
                        digits += 1;
                        value = value * 16 + ch.to_digit(16).unwrap();
                    }
                    _ => return Err(LiteralError::InvalidUnicodeEscape),
                }
            }
            char::from_u32(value).ok_or(LiteralError::InvalidUnicodeScalar)
        }
        _ => Err(LiteralError::InvalidCharEscape),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_characters_require_exactly_one_ascii_or_escaped_byte() {
        for (text, expected) in [
            ("b'A'", b'A'),
            (r"b'\0'", 0),
            (r"b'\n'", b'\n'),
            (r"b'\r'", b'\r'),
            (r"b'\t'", b'\t'),
            (r"b'\''", b'\''),
            (r"b'\\'", b'\\'),
            (r"b'\x00'", 0),
            (r"b'\x7F'", 127),
            (r"b'\x80'", 128),
            (r"b'\xFF'", 255),
        ] {
            assert_eq!(decode_byte_char_literal(text), Ok(expected), "{text}");
        }
        for (text, expected) in [
            ("b''", LiteralError::EmptyChar),
            ("b'ab'", LiteralError::ByteCharWrongLength),
            ("b'é'", LiteralError::NonAsciiByteText),
            ("b'🙂'", LiteralError::NonAsciiByteText),
            ("b'\t'", LiteralError::DirectCharWhitespace),
            ("b'\n'", LiteralError::DirectCharWhitespace),
            ("b'\r'", LiteralError::DirectCharWhitespace),
            ("b'''", LiteralError::UnescapedApostrophe),
            ("b'", LiteralError::UnterminatedChar),
            (r"b'\x0'", LiteralError::InvalidAsciiEscape),
            (r"b'\xGG'", LiteralError::InvalidAsciiEscape),
            (r"b'\u{41}'", LiteralError::UnicodeEscapeInByteLiteral),
            (r"b'\u{FF}'", LiteralError::UnicodeEscapeInByteLiteral),
            (r"b'\q'", LiteralError::InvalidCharEscape),
            ("'A'", LiteralError::InvalidByteCharPrefix),
        ] {
            assert_eq!(decode_byte_char_literal(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn byte_strings_share_delimiters_and_lines_without_utf8_or_interpolation() {
        for (text, expected) in [
            (r#"b"\0\x00\x7F\x80\xFF""#, &[0, 0, 127, 128, 255][..]),
            (r#"b"{name} {{}} } {""#, b"{name} {{}} } {".as_slice()),
            (
                r##"br#"\xFF\u{41}{name}""#"##,
                b"\\xFF\\u{41}{name}\"".as_slice(),
            ),
            (
                "b\"\r\n  first\r\nsecond\n\"",
                b"\n  first\nsecond\n".as_slice(),
            ),
            ("b\"a\\\r\n\r \t\nb\"", b"ab".as_slice()),
            ("b\"a\\\n \tb\"", b"ab".as_slice()),
            ("br\"a\\\r\n b\"", b"a\\\n b".as_slice()),
            ("b\"\"", b"".as_slice()),
            ("br\"\"", b"".as_slice()),
        ] {
            assert_eq!(
                decode_byte_string_literal(text).as_deref(),
                Ok(expected),
                "{text:?}"
            );
        }
        for count in [0, 1, 2, 255] {
            let hashes = "#".repeat(count);
            let text = format!("br{hashes}\"{{name}}\\n\"{hashes}");
            assert_eq!(decode_byte_string_literal(&text).unwrap(), b"{name}\\n");
        }
        assert_eq!(
            decode_byte_string_literal(&format!("br{}\"x\"", "#".repeat(256))),
            Err(LiteralError::TooManyRawStringHashes)
        );
    }

    #[test]
    fn byte_strings_reject_non_ascii_unicode_escapes_and_malformed_text() {
        for (text, expected) in [
            (r#"b"é""#, LiteralError::NonAsciiByteText),
            (r#"b"中文""#, LiteralError::NonAsciiByteText),
            (r#"br"🙂""#, LiteralError::NonAsciiByteText),
            (r##"br#"é"#"##, LiteralError::NonAsciiByteText),
            (r#"b"\u{41}""#, LiteralError::UnicodeEscapeInByteLiteral),
            (r#"b"\u{FF}""#, LiteralError::UnicodeEscapeInByteLiteral),
            (r#"b"\u{110000}""#, LiteralError::UnicodeEscapeInByteLiteral),
            (r#"b"\x0""#, LiteralError::InvalidAsciiEscape),
            (r#"b"\xGG""#, LiteralError::InvalidAsciiEscape),
            (r#"b"\q""#, LiteralError::InvalidCharEscape),
            (r#"b"\{""#, LiteralError::InvalidCharEscape),
            ("b\"a\rb\"", LiteralError::StandaloneCarriageReturn),
            ("br\"a\rb\"", LiteralError::StandaloneCarriageReturn),
            ("b\"a\\\rb\"", LiteralError::InvalidStringContinuation),
            ("b\"unterminated", LiteralError::UnterminatedString),
            ("br#\"x\"", LiteralError::UnterminatedString),
            ("br#\"x\"##", LiteralError::UnterminatedString),
            ("b\"a\"b\"", LiteralError::UnescapedStringQuote),
            (
                "br\"a\"b\"",
                LiteralError::UnexpectedTextAfterRawStringDelimiter,
            ),
            ("\"A\"", LiteralError::InvalidByteStringPrefix),
        ] {
            assert_eq!(decode_byte_string_literal(text), Err(expected), "{text:?}");
        }
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert_eq!(
            decode_byte_string_literal_with_cancellation("b\"text\"", &cancel),
            Err(Cancelled)
        );
    }

    #[test]
    fn string_literals_decode_escapes_and_source_braces_once() {
        for (text, expected) in [
            (r#""\x00\x7F\u{10_FF_FF_}""#, "\0\x7f\u{10ffff}"),
            (r#""\u{7B}name\u{7D}""#, "{name}"),
            (r#""{{name}} {{}}""#, "{name} {}"),
            (r#""\\u{{7B}}""#, r"\u{7B}"),
            (r#""\"\'\\\n\r\t\0""#, "\"'\\\n\r\t\0"),
            (r##"r"\n{name}""##, r"\n{name}"),
            (r###"r#"quote: " and \u{7B}"#"###, "quote: \" and \\u{7B}"),
        ] {
            assert_eq!(
                decode_string_literal(text).as_deref(),
                Ok(expected),
                "{text}"
            );
        }
        assert_eq!(
            decode_string_fragment(r"{{\u{7B}\u{7D}}}"),
            Ok("{{}}".into())
        );
    }

    #[test]
    fn string_literals_normalize_only_physical_crlf_and_remove_cooked_continuations() {
        for (text, expected) in [
            ("\"\r\n  first\r\nsecond\n\"", "\n  first\nsecond\n"),
            ("r#\"\r\n  first\r\n\"#", "\n  first\n"),
            ("\"a\\\n \t\r\nb\"", "ab"),
            ("\"a\\\r\n\r \t\nb\"", "ab"),
            ("r\"a\\\r\n b\"", "a\\\n b"),
            (r#""a\n b\r""#, "a\n b\r"),
        ] {
            assert_eq!(
                decode_string_literal(text).as_deref(),
                Ok(expected),
                "{text:?}"
            );
        }
        for (text, expected) in [
            ("\"a\rb\"", LiteralError::StandaloneCarriageReturn),
            ("r\"a\rb\"", LiteralError::StandaloneCarriageReturn),
            ("\"a\\\rb\"", LiteralError::InvalidStringContinuation),
        ] {
            assert_eq!(decode_string_literal(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn string_literals_reject_holes_invalid_escapes_and_delimiters() {
        for (text, expected) in [
            (r#""{name}""#, LiteralError::UndoubledStringBrace),
            (r#""{name:?}""#, LiteralError::UndoubledStringBrace),
            (r#""}""#, LiteralError::UndoubledStringBrace),
            (r#""{""#, LiteralError::UndoubledStringBrace),
            (r#""\{""#, LiteralError::InvalidCharEscape),
            (r#""\x80""#, LiteralError::NonAsciiHexEscape),
            (r#""\xFF""#, LiteralError::NonAsciiHexEscape),
            (r#""\x0""#, LiteralError::InvalidAsciiEscape),
            (r#""\u{}""#, LiteralError::InvalidUnicodeEscape),
            (r#""\u{_1}""#, LiteralError::InvalidUnicodeEscape),
            (r#""\u{0000000}""#, LiteralError::InvalidUnicodeEscape),
            (r#""\u{D800}""#, LiteralError::InvalidUnicodeScalar),
            (r#""\u{110000}""#, LiteralError::InvalidUnicodeScalar),
            (r#""\q""#, LiteralError::InvalidCharEscape),
            (r#"f"old""#, LiteralError::InvalidStringOpening),
            (r##"r#"missing""##, LiteralError::UnterminatedString),
            (r#""a"b""#, LiteralError::UnescapedStringQuote),
            (
                r##"r"a"b""##,
                LiteralError::UnexpectedTextAfterRawStringDelimiter,
            ),
            ("\"🙂", LiteralError::UnterminatedString),
            ("r\"🙂", LiteralError::UnterminatedString),
        ] {
            assert_eq!(decode_string_literal(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn string_decoding_does_not_publish_cancelled_values() {
        let cancel = CancellationToken::default();
        let incomplete_fragment = "\\";
        assert_eq!(
            decode_string_fragment_with_cancellation(incomplete_fragment, &cancel),
            Ok(Err(LiteralError::IncompleteStringEscape))
        );
        cancel.cancel();
        assert_eq!(
            decode_string_literal_with_cancellation("\"text\"", &cancel),
            Err(Cancelled)
        );
        assert_eq!(
            decode_string_fragment_with_cancellation(incomplete_fragment, &cancel),
            Err(Cancelled)
        );

        // Request cancellation from the first decoded character, after scanning
        // has begun. Long text, escapes and continuations must all stop early.
        for content in [
            "值a".repeat(10_000),
            format!("a\\u{{1{}}}b", "_".repeat(40_000)),
            format!("a\\\n{}b", " ".repeat(40_000)),
        ] {
            let cancel = CancellationToken::default();
            let mut decoded = String::new();
            let result = decode_string_content(&content, true, false, &cancel, &mut |ch| {
                decoded.push(ch);
                cancel.cancel();
            });
            assert_eq!(result, Err(Cancelled));
            assert!(cancel.check().is_err());
            assert!(decoded.len() < content.len());
            assert!(!decoded.ends_with('b'));
        }
    }

    #[test]
    fn string_literals_raw_delimiters_are_bounded_and_close_before_extra_hashes() {
        for hashes in [0, 1, 2, 255] {
            let hashes = "#".repeat(hashes);
            let text = format!("r{hashes}\"{{name}}\\n\"{hashes}");
            assert_eq!(decode_string_literal(&text), Ok("{name}\\n".into()));
        }
        assert_eq!(
            StringDelimiter::opening(&format!("r{}\"x\"", "#".repeat(256))),
            Err(LiteralError::TooManyRawStringHashes)
        );
        assert_eq!(
            StringDelimiter::opening("r#missing"),
            Err(LiteralError::InvalidRawStringOpening)
        );
        assert_eq!(StringDelimiter::Raw(2).closing_len("\"###"), Some(3));
        assert_eq!(StringDelimiter::Raw(2).closing_len("\"#x"), None);
        assert_eq!(
            decode_string_literal("r#\"x\"##"),
            Err(LiteralError::UnterminatedString)
        );
    }

    #[test]
    fn character_literals_are_exactly_one_unicode_scalar() {
        for (text, expected) in [
            ("'a'", 'a'),
            ("'🙂'", '🙂'),
            (r"'\0'", '\0'),
            (r"'\n'", '\n'),
            (r"'\r'", '\r'),
            (r"'\t'", '\t'),
            (r"'\''", '\''),
            (r"'\\'", '\\'),
            (r"'\x7F'", '\x7f'),
            (r"'\u{0}'", '\0'),
            (r"'\u{10_FF_FF_}'", '\u{10ffff}'),
            (r"'\u{D7FF}'", '\u{d7ff}'),
            (r"'\u{E000}'", '\u{e000}'),
        ] {
            assert_eq!(decode_char_literal(text), Ok(expected), "{text}");
        }
        for (text, expected) in [
            ("''", LiteralError::EmptyChar),
            ("'ab'", LiteralError::CharWrongLength),
            ("'á'", LiteralError::CharWrongLength),
            ("'", LiteralError::UnterminatedChar),
            ("'''", LiteralError::UnescapedApostrophe),
            ("'\t'", LiteralError::DirectCharWhitespace),
            ("'\n'", LiteralError::DirectCharWhitespace),
            ("'\r'", LiteralError::DirectCharWhitespace),
            (r"'\x80'", LiteralError::NonAsciiHexEscape),
            (r"'\xFF'", LiteralError::NonAsciiHexEscape),
            (r"'\x0'", LiteralError::InvalidAsciiEscape),
            (r"'\u{}'", LiteralError::InvalidUnicodeEscape),
            (r"'\u{_1}'", LiteralError::InvalidUnicodeEscape),
            (r"'\u{0000000}'", LiteralError::InvalidUnicodeEscape),
            (r"'\u{D800}'", LiteralError::InvalidUnicodeScalar),
            (r"'\u{DFFF}'", LiteralError::InvalidUnicodeScalar),
            (r"'\u{110000}'", LiteralError::InvalidUnicodeScalar),
            (r"'\q'", LiteralError::InvalidCharEscape),
            (r"'\u{1'", LiteralError::InvalidUnicodeEscape),
        ] {
            assert_eq!(decode_char_literal(text), Err(expected), "{text:?}");
        }
    }

    #[test]
    fn integer_suffixes_are_fixed_width() {
        for suffix in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
            let text = format!("42{suffix}");
            assert_eq!(numeric_literal_parts(&text), ("42", Some(suffix)));
            assert!(is_integer_literal(&text));
            assert_eq!(parse_integer_literal(&text), Ok(42));
        }
        for suffix in ["isize", "usize"] {
            for digits in ["42", "0x2a", "0o52", "0b101010"] {
                let text = format!("{digits}{suffix}");
                assert_eq!(numeric_literal_parts(&text), (text.as_str(), None));
                assert!(!is_integer_literal(&text));
                assert_eq!(
                    parse_integer_literal(&text),
                    Err(LiteralError::InvalidIntegerLiteral)
                );
            }
        }
        assert_eq!(
            parse_integer_literal("18446744073709551615u64"),
            Ok(u64::MAX)
        );
        let too_large = "18446744073709551616u64";
        assert!(is_integer_literal(too_large));
        assert_eq!(
            parse_integer_literal(too_large),
            Err(LiteralError::IntegerOutOfRange)
        );
        assert_eq!(parse_integer_literal("0xf64"), Ok(0xf64));
        assert_eq!(
            parse_integer_literal("42f64"),
            Err(LiteralError::FloatSuffixOnInteger)
        );
        assert_eq!(
            LiteralError::InvalidIntegerLiteral.to_string(),
            "invalid integer literal"
        );
    }
}
