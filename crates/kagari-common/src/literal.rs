/// Parse an unsigned integer literal before applying any unary sign or target-type bound.
pub fn parse_integer_literal(text: &str) -> Result<u64, &'static str> {
    let (radix, digits) = integer_digits(text)?;
    let compact = digits.chars().filter(|ch| *ch != '_').collect::<String>();
    u64::from_str_radix(&compact, radix).map_err(|_| "integer literal is outside the u64 range")
}

/// Check grammar without imposing a machine-integer size on the lexer.
pub fn is_integer_literal(text: &str) -> bool {
    integer_digits(text).is_ok()
}

fn integer_digits(text: &str) -> Result<(u32, &str), &'static str> {
    let (radix, digits) = if let Some(digits) = text.strip_prefix("0b") {
        (2, digits)
    } else if let Some(digits) = text.strip_prefix("0o") {
        (8, digits)
    } else if let Some(digits) = text.strip_prefix("0x") {
        (16, digits)
    } else {
        (10, text)
    };
    if digits.is_empty() || !digits.chars().next().is_some_and(|ch| ch.is_digit(radix)) {
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
