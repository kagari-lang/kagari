use std::iter;

use kagari_common::cancellation::{CancellationToken, Cancelled};

/// Parse an unsigned integer literal before applying any unary sign or target-type bound.
pub fn parse_integer_literal(text: &str) -> Result<u64, &'static str> {
    let (radix, digits) = integer_digits(text)?;
    let compact = digits.chars().filter(|ch| *ch != '_').collect::<String>();
    u64::from_str_radix(&compact, radix).map_err(|_| "integer literal is outside the u64 range")
}

/// Separate a primitive type suffix without confusing hexadecimal digits with f32/f64.
pub fn numeric_literal_parts(text: &str) -> (&str, Option<&str>) {
    for suffix in [
        "i64", "u64", "i32", "u32", "i16", "u16", "i8", "u8", "f32", "f64",
    ] {
        if suffix.starts_with('f')
            && ["0x", "0o", "0b"]
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

fn integer_digits(text: &str) -> Result<(u32, &str), &'static str> {
    let (text, suffix) = numeric_literal_parts(text);
    if suffix.is_some_and(|suffix| suffix.starts_with('f')) {
        return Err("floating-point suffix on integer literal");
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
        return Err("invalid integer literal");
    }
    if !digits.chars().all(|ch| ch == '_' || ch.is_digit(radix)) {
        return Err("invalid integer literal");
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
    pub fn opening(text: &str) -> Result<Option<(Self, usize)>, &'static str> {
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
                        .ok_or("raw String has more than 255 hashes")?
                }
                _ => break,
            }
        }
        Err("invalid raw String opening delimiter")
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
pub fn decode_string_literal(text: &str) -> Result<String, &'static str> {
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
) -> Result<Result<String, &'static str>, Cancelled> {
    cancel.check()?;
    let decoded = decode_string_literal_inner(text, cancel);
    cancel.check()?;
    Ok(decoded)
}

fn decode_string_literal_inner(
    text: &str,
    cancel: &CancellationToken,
) -> Result<String, &'static str> {
    let (delimiter, opening) =
        StringDelimiter::opening(text)?.ok_or("invalid String opening delimiter")?;
    let closing = match delimiter {
        StringDelimiter::Cooked => 1,
        StringDelimiter::Raw(hashes) => usize::from(hashes) + 1,
    };
    let end = text
        .len()
        .checked_sub(closing)
        .filter(|end| *end >= opening)
        .ok_or("unterminated String literal")?;
    if text
        .get(end..)
        .and_then(|suffix| delimiter.closing_len(suffix))
        != Some(closing)
    {
        return Err("unterminated String literal");
    }
    let content = text
        .get(opening..end)
        .ok_or("invalid String delimiter boundary")?;
    match delimiter {
        StringDelimiter::Cooked => decode_string_content(content, true, cancel),
        StringDelimiter::Raw(_) => {
            for (index, ch) in content.char_indices() {
                if cancel.check().is_err() {
                    return Err("String decoding cancelled");
                }
                if ch == '"' && delimiter.closing_len(&content[index..]).is_some() {
                    return Err("unexpected text after raw String delimiter");
                }
            }
            decode_string_content(content, false, cancel)
        }
    }
}

/// Decode one source text fragment between interpolation holes, without re-quoting.
///
/// Escapes are consumed before source braces: `\u{7B}` produces literal `{`.
/// A fragment cannot contain a hole, lone brace, unescaped quote or invalid escape.
/// Continuations remove the newline and following ASCII space/tab/CR/LF.
pub fn decode_string_fragment(content: &str) -> Result<String, &'static str> {
    decode_string_fragment_with_cancellation(content, &CancellationToken::default())
        .expect("fresh cancellation token")
}

/// Decode a source fragment with the same cancellation/error contract as the full literal decoder.
pub fn decode_string_fragment_with_cancellation(
    content: &str,
    cancel: &CancellationToken,
) -> Result<Result<String, &'static str>, Cancelled> {
    cancel.check()?;
    let decoded = decode_string_content(content, true, cancel);
    cancel.check()?;
    Ok(decoded)
}

fn decode_string_content(
    content: &str,
    cooked: bool,
    cancel: &CancellationToken,
) -> Result<String, &'static str> {
    let mut decoded = String::with_capacity(content.len());
    let mut chars = content
        .chars()
        .take_while(|_| cancel.check().is_ok())
        .peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.next() != Some('\n') {
                    return Err("standalone CR in String literal");
                }
                decoded.push('\n');
            }
            '\\' if cooked => {
                let next = chars.next().ok_or("incomplete String escape")?;
                if matches!(next, '\n' | '\r') {
                    if next == '\r' && chars.next() != Some('\n') {
                        return Err("String continuation requires LF or CRLF");
                    }
                    while chars
                        .peek()
                        .is_some_and(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n'))
                    {
                        chars.next();
                    }
                } else {
                    decoded.push(decode_char_escape(
                        &mut iter::once(next).chain(chars.by_ref()),
                    )?);
                }
            }
            '{' | '}' if cooked => {
                if chars.next() != Some(ch) {
                    return Err(
                        "String text requires doubled braces; interpolation requires a name binding",
                    );
                }
                decoded.push(ch);
            }
            '"' if cooked => return Err("unescaped quote in String text"),
            _ => decoded.push(ch),
        }
    }
    Ok(decoded)
}

/// Decode a single-quoted character literal to exactly one Unicode scalar.
///
/// Escapes follow Rust's character spelling: ASCII `\xNN`, one to six hex
/// digits in `\u{...}` (underscores only after a digit), and simple escapes.
/// Direct tabs/newlines and empty or multiple-scalar contents are errors.
pub fn decode_char_literal(text: &str) -> Result<char, &'static str> {
    let content = text
        .strip_prefix('\'')
        .and_then(|text| text.strip_suffix('\''))
        .ok_or("unterminated char literal")?;
    let mut chars = content.chars();
    let value = match chars.next() {
        Some('\\') => decode_char_escape(&mut chars)?,
        Some('\t' | '\r' | '\n') => return Err("direct whitespace in char literal"),
        Some('\'') => return Err("unescaped apostrophe in char literal"),
        Some(value) => value,
        None => return Err("empty char literal"),
    };
    if chars.next().is_some() {
        return Err("char literal must contain exactly one Unicode scalar");
    }
    Ok(value)
}

fn decode_char_escape(chars: &mut impl Iterator<Item = char>) -> Result<char, &'static str> {
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
                    .ok_or("invalid ASCII escape")?;
                value = value * 16 + digit;
            }
            if value > 0x7f {
                return Err("char hexadecimal escape must be ASCII");
            }
            char::from_u32(value).ok_or("invalid Unicode scalar")
        }
        Some('u') => {
            if chars.next() != Some('{') {
                return Err("invalid Unicode escape");
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
                    _ => return Err("invalid Unicode escape"),
                }
            }
            char::from_u32(value).ok_or("invalid Unicode scalar")
        }
        _ => Err("invalid char escape"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        for text in ["\"a\rb\"", "r\"a\rb\"", "\"a\\\rb\""] {
            assert!(decode_string_literal(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn string_literals_reject_holes_invalid_escapes_and_delimiters() {
        for text in [
            r#""{name}""#,
            r#""{name:?}""#,
            r#""}""#,
            r#""{""#,
            r#""\{""#,
            r#""\x80""#,
            r#""\xFF""#,
            r#""\x0""#,
            r#""\u{}""#,
            r#""\u{_1}""#,
            r#""\u{0000000}""#,
            r#""\u{D800}""#,
            r#""\u{110000}""#,
            r#""\q""#,
            r#"f"old""#,
            r##"r#"missing""##,
            r#""a"b""#,
            r##"r"a"b""##,
            "\"🙂",
            "r\"🙂",
        ] {
            assert!(decode_string_literal(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn string_decoding_does_not_publish_cancelled_values() {
        let cancel = CancellationToken::default();
        cancel.cancel();
        assert!(decode_string_literal_with_cancellation("\"text\"", &cancel).is_err());
        assert!(decode_string_fragment_with_cancellation("text", &cancel).is_err());
    }

    #[test]
    fn string_literals_raw_delimiters_are_bounded_and_close_before_extra_hashes() {
        for hashes in [0, 1, 2, 255] {
            let hashes = "#".repeat(hashes);
            let text = format!("r{hashes}\"{{name}}\\n\"{hashes}");
            assert_eq!(decode_string_literal(&text), Ok("{name}\\n".into()));
        }
        assert!(StringDelimiter::opening(&format!("r{}\"x\"", "#".repeat(256))).is_err());
        assert_eq!(StringDelimiter::Raw(2).closing_len("\"###"), Some(3));
        assert_eq!(StringDelimiter::Raw(2).closing_len("\"#x"), None);
        assert!(decode_string_literal("r#\"x\"##").is_err());
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
        for text in [
            "''",
            "'ab'",
            "'á'",
            "'",
            "'''",
            "'\t'",
            "'\n'",
            "'\r'",
            r"'\x80'",
            r"'\xFF'",
            r"'\x0'",
            r"'\u{}'",
            r"'\u{_1}'",
            r"'\u{0000000}'",
            r"'\u{D800}'",
            r"'\u{DFFF}'",
            r"'\u{110000}'",
            r"'\q'",
            r"'\u{1'",
        ] {
            assert!(decode_char_literal(text).is_err(), "{text:?}");
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
                assert_eq!(parse_integer_literal(&text), Err("invalid integer literal"));
            }
        }
        assert_eq!(
            parse_integer_literal("18446744073709551615u64"),
            Ok(u64::MAX)
        );
        assert_eq!(parse_integer_literal("0xf64"), Ok(0xf64));
    }
}
