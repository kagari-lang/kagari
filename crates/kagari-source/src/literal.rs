use std::str::Chars;

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

/// Decode a quoted source string, rejecting invalid escapes and Unicode scalars.
pub fn decode_string_literal(text: &str) -> Result<String, &'static str> {
    let content = text
        .strip_prefix('"')
        .and_then(|text| text.strip_suffix('"'))
        .ok_or("unterminated String literal")?;
    let mut decoded = String::with_capacity(content.len());
    let mut chars = content.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' | '\n' => return Err("line break in String literal"),
            '\\' => match chars.next() {
                Some('"') => decoded.push('"'),
                Some('\\') => decoded.push('\\'),
                Some('n') => decoded.push('\n'),
                Some('r') => decoded.push('\r'),
                Some('t') => decoded.push('\t'),
                Some('0') => decoded.push('\0'),
                Some('u') => {
                    if chars.next() != Some('{') {
                        return Err("invalid Unicode escape");
                    }
                    let mut scalar = 0u32;
                    let mut saw_digit = false;
                    loop {
                        match chars.next() {
                            Some('}') if saw_digit => break,
                            Some(ch) if ch.is_ascii_hexdigit() => {
                                saw_digit = true;
                                scalar = scalar
                                    .checked_mul(16)
                                    .and_then(|value| value.checked_add(ch.to_digit(16).unwrap()))
                                    .ok_or("invalid Unicode scalar")?;
                            }
                            _ => return Err("invalid Unicode escape"),
                        }
                    }
                    decoded.push(char::from_u32(scalar).ok_or("invalid Unicode scalar")?);
                }
                _ => return Err("invalid String escape"),
            },
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

fn decode_char_escape(chars: &mut Chars<'_>) -> Result<char, &'static str> {
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
