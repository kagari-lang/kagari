//! Single-line escaping shared by names and quoted artifact annotations.

use std::fmt::Write;

/// Keeps ordinary path components readable without allowing syntax delimiters.
pub(super) fn name(text: &str) -> String {
    if !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        text.to_owned()
    } else {
        let mut output = String::new();
        push_quoted(&mut output, text);
        output
    }
}

pub(super) fn push_quoted(output: &mut String, text: &str) {
    output.push('"');
    for ch in text.chars() {
        match ch {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            '\0' => output.push_str("\\0"),
            ch if ch.is_control() || is_format_or_separator(ch) => {
                write!(output, "\\u{{{:x}}}", u32::from(ch)).expect("writing to a String");
            }
            ch => output.push(ch),
        }
    }
    output.push('"');
}

/// Unicode format controls (General_Category=Cf), plus line/paragraph separators.
///
/// Keep combining marks and ordinary non-ASCII text intact rather than applying
/// `escape_debug`, whose printable-character policy also escapes ordinary text.
fn is_format_or_separator(ch: char) -> bool {
    matches!(
        ch,
        '\u{ad}'
            | '\u{600}'..='\u{605}'
            | '\u{61c}'
            | '\u{6dd}'
            | '\u{70f}'
            | '\u{890}'..='\u{891}'
            | '\u{8e2}'
            | '\u{180e}'
            | '\u{200b}'..='\u{200f}'
            | '\u{2028}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{206f}'
            | '\u{feff}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{110bd}'
            | '\u{110cd}'
            | '\u{13430}'..='\u{1343f}'
            | '\u{1bca0}'..='\u{1bca3}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}
