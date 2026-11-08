//! UTF-8 contracts with concrete, executable observations.
use crate::catalog::documentation::example;

pub(super) const OVERVIEW: (&str, &str) = (
    "Immutable UTF-8 strings.\n\nLengths and positions are byte counts. Use `chars` for Unicode scalar values,\n`char_indices` for their byte positions, and `bytes` for the UTF-8 encoding.\nA scalar is not necessarily a whole displayed character: combining marks are\nseparate scalars. String itself is not Iterable and has no indexing operator.\n\nTransformations return strings and leave the input unchanged. `slice` requires\nordered offsets on UTF-8 boundaries; `find` returns a byte offset or `None`.",
    "fn main() -> bool {\n    val input = \"  café  \";\n    input.trim() == \"café\" && input == \"  café  \" && \"é\".len() == 2u64\n}",
);

pub(super) fn method(name: &str) -> String {
    let (description, code) = match name {
        "len" => (
            "Returns the number of UTF-8 bytes.\n\nThis is not the number of Unicode scalars or displayed characters. Empty\nstrings have length zero. Use `chars().count()` to count scalars.",
            "fn main() -> bool {\n    \"aé🙂\".len() == 7u64 && \"aé🙂\".chars().count() == 3u64 && \"\".len() == 0u64\n}",
        ),
        "is_empty" => (
            "Returns whether the string contains no bytes.\n\nWhitespace and the NUL scalar are content, so neither makes a string empty.",
            "fn main() -> bool {\n    \"\".is_empty() && !\" \".is_empty() && !\"\\0\".is_empty()\n}",
        ),
        "chars" => (
            "Returns a lazy iterator over Unicode scalar values.\n\nCombining marks and NUL are separate items. Each call creates a fresh cursor\nretaining this immutable string. Cursor copies share progress; exhausted\ncursors return `None`. No intermediate collection is created.",
            "fn main() -> bool {\n    val cursor = \"e\\u{301}🙂\".chars();\n    cursor.next() == Some('e') && cursor.next() == Some('\\u{301}') &&\n        cursor.next() == Some('🙂') && cursor.next() == None\n}",
        ),
        "char_indices" => (
            "Returns a lazy iterator of `(byte_offset, scalar)` pairs.\n\nOffsets are zero-based UTF-8 byte positions, not scalar ordinals. Combining\nmarks remain separate scalars rather than being joined into grapheme clusters.\nAn empty string yields no pairs.\n\nEach call creates a fresh cursor retaining this immutable string. Cursor\ncopies share progress. Exhaustion returns `None` without creating a List.",
            "fn main() -> bool {\n    val cursor = \"aé🙂\".char_indices();\n    cursor.next() == Some((0u64, 'a')) &&\n        cursor.next() == Some((1u64, 'é')) &&\n        cursor.next() == Some((3u64, '🙂')) && cursor.next() == None &&\n        \"\".char_indices().next() == None\n}",
        ),
        "bytes" => (
            "Returns a lazy iterator over the UTF-8 encoding as `u8` values.\n\nA non-ASCII scalar can occupy several bytes. Each call creates a fresh cursor\nretaining the string; cursor copies share progress. Empty or exhausted cursors\nreturn `None`. Use `chars` when the desired items are Unicode scalars.",
            "fn main() -> bool {\n    val cursor = \"é\".bytes();\n    cursor.next() == Some(195u8) && cursor.next() == Some(169u8) &&\n        cursor.next() == None && \"\".bytes().next() == None\n}",
        ),
        "contains" => (
            "Returns whether `pattern` occurs as a literal substring.\n\nPatterns are case-sensitive strings, not regular expressions. An empty\npattern matches every string, including the empty string.",
            "fn main() -> bool {\n    \"café\".contains(\"fé\") && !\"café\".contains(\"FÉ\") && \"\".contains(\"\")\n}",
        ),
        "starts_with" => (
            "Returns whether this string begins with the literal `pattern`.\n\nThe match is case-sensitive. An empty pattern matches; a longer pattern\ncannot match a shorter string.",
            "fn main() -> bool {\n    \"café\".starts_with(\"ca\") && !\"café\".starts_with(\"fé\") && \"\".starts_with(\"\")\n}",
        ),
        "ends_with" => (
            "Returns whether this string ends with the literal `pattern`.\n\nThe match is case-sensitive. An empty pattern matches every string.",
            "fn main() -> bool {\n    \"café\".ends_with(\"fé\") && !\"café\".ends_with(\"ca\") && \"\".ends_with(\"\")\n}",
        ),
        "find" => (
            "Returns the first literal match's byte offset, or `None`.\n\nThe returned `u64` is a UTF-8 byte position, not a scalar ordinal. An empty\npattern returns `Some(0u64)`, even for an empty receiver.",
            "fn main() -> bool {\n    \"aé🙂é\".find(\"🙂\") == Some(3u64) &&\n        \"aé🙂é\".find(\"é\") == Some(1u64) &&\n        \"aé🙂é\".find(\"x\") == None && \"\".find(\"\") == Some(0u64)\n}",
        ),
        "slice" => (
            "Copies the half-open byte range `start..end` into a string.\n\nThe receiver is unchanged. Equal valid offsets produce an empty string.\n\n# Traps\n\nTraps when `start > end`, either offset exceeds the byte length, or either\noffset lies inside a multibyte UTF-8 scalar. `None` is not returned for these\ninvalid ranges. Calling `invalid_boundary` below traps with IndexOutOfBounds:\nbyte 2 lies inside the encoding of é. The normal main checks valid slices.",
            "fn main() -> bool {\n    val input = \"aé🙂\";\n    input.slice(1u64, 3u64) == \"é\" && input.slice(3u64, 7u64) == \"🙂\" &&\n        input.slice(1u64, 1u64) == \"\" && input == \"aé🙂\"\n}\n\npub fn invalid_boundary() -> bool {\n    val invalid = \"aé🙂\".slice(2u64, 3u64);\n    invalid.is_empty()\n}",
        ),
        "split" => (
            "Splits on a literal separator and returns an eager `List<String>`.\n\nFields retain source order. Adjacent separators and separators at either end\nproduce empty fields. An empty separator splits at Unicode scalar boundaries\nand includes an empty field at each end. This operation is not lazy.",
            "fn main() -> bool {\n    val fields = \",a,,b,\".split(\",\");\n    val scalars = \"é🙂\".split(\"\");\n    fields.len() == 5u64 && fields.get(0u64) == Some(\"\") &&\n        fields.get(2u64) == Some(\"\") && fields.get(4u64) == Some(\"\") &&\n        scalars.len() == 4u64 && scalars.get(1u64) == Some(\"é\") &&\n        scalars.get(2u64) == Some(\"🙂\")\n}",
        ),
        "replace" => (
            "Replaces every non-overlapping literal `from` match with `to`.\n\nReturns a string without modifying the receiver. Matches are case-sensitive.\nAn empty `from` inserts `to` at every Unicode scalar boundary, including both\nends. Replacements are not searched again.",
            "fn main() -> bool {\n    val input = \"aaaaa\";\n    input.replace(\"aa\", \"b\") == \"bba\" && input == \"aaaaa\" &&\n        \"é\".replace(\"\", \"-\") == \"-é-\" && \"abc\".replace(\"x\", \"y\") == \"abc\"\n}",
        ),
        "trim" => (
            "Removes Unicode whitespace from both ends.\n\nInterior whitespace is retained and the receiver is unchanged. Empty strings\nand strings consisting only of whitespace produce an empty string.",
            "fn main() -> bool {\n    val input = \"\\t café au lait \\n\";\n    input.trim() == \"café au lait\" && input.starts_with(\"\\t\") &&\n        \" \\t\".trim() == \"\"\n}",
        ),
        "trim_start" => (
            "Removes leading Unicode whitespace.\n\nInterior and trailing whitespace are retained. The receiver is unchanged.",
            "fn main() -> bool {\n    \" \\t café \\n\".trim_start() == \"café \\n\" && \"\".trim_start() == \"\"\n}",
        ),
        "trim_end" => (
            "Removes trailing Unicode whitespace.\n\nInterior and leading whitespace are retained. The receiver is unchanged.",
            "fn main() -> bool {\n    \" \\t café \\n\".trim_end() == \" \\t café\" && \"\".trim_end() == \"\"\n}",
        ),
        _ => unreachable!("authored String operation inventory"),
    };
    example(description, code)
}
