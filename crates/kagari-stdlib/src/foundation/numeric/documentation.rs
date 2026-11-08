//! Concrete scalar parsing and conversion witnesses, shared by semantic family.
use crate::catalog::documentation::example;
use kagari_types::scalar::BuiltinType;

pub(super) fn conversion(source: BuiltinType, target: BuiltinType, fallible: bool) -> String {
    let source_name = format!("{source:?}").to_lowercase();
    let target_name = format!("{target:?}").to_lowercase();
    let input = if source == BuiltinType::Bool {
        "true".into()
    } else {
        format!("42{source_name}")
    };
    let output = if target == BuiltinType::Bool {
        "true".into()
    } else if source == BuiltinType::Bool {
        format!("1{target_name}")
    } else {
        format!("42{target_name}")
    };
    let success = format!(
        "match {target_name}::try_from({input}) {{\n        Ok(value) => value == {output},\n        Err(_) => false,\n    }}"
    );
    let (description, code) = if fallible {
        let (source_bits, source_signed) = source.integer_layout().unwrap();
        let (target_bits, target_signed) = target.integer_layout().unwrap();
        let invalid = if source_signed && !target_signed {
            format!("-1{source_name}")
        } else {
            let value = if target_signed {
                1u128 << (target_bits - 1)
            } else {
                1u128 << target_bits
            };
            assert!(source_bits > target_bits || (!source_signed && target_signed));
            format!("{value}{source_name}")
        };
        (
            format!(
                "Converts `{source_name}` to `{target_name}` when the value fits.\n\nReturns `TryFromIntError::OutOfRange` when the value is outside the target's\nrepresentable range. No wrapping or truncation is performed."
            ),
            format!(
                "use core::num::TryFromIntError;\nfn main() -> bool {{\n    val fits = {success};\n    val rejected = match {target_name}::try_from({invalid}) {{\n        Err(TryFromIntError::OutOfRange) => true,\n        _ => false,\n    }};\n    fits && rejected\n}}"
            ),
        )
    } else {
        (
            format!(
                "Converts `{source_name}` to `{target_name}` without losing information.\n\nEvery source value is representable. The declared error type is `Infallible`;\nthe result always contains `Ok`. Boolean conversion uses zero for false and one\nfor true when the target is an integer."
            ),
            format!("fn main() -> bool {{\n    {success}\n}}"),
        )
    };
    example(&description, &code)
}

pub(super) fn parsing(target: BuiltinType) -> String {
    let name = format!("{target:?}").to_lowercase();
    let (input, output, invalid, error, detail): (&str, String, &str, &str, &str) = match target {
        BuiltinType::Bool => (
            "true",
            "true".into(),
            "TRUE",
            "InvalidSyntax",
            "Accepts exactly `true` and `false`, in lowercase.",
        ),
        BuiltinType::F32 | BuiltinType::F64 => (
            "1.25",
            format!("1.25{name}"),
            "1.2.3",
            "InvalidSyntax",
            "Accepts decimal and exponent notation, NaN and infinity. Invalid syntax returns\n`InvalidSyntax`; a finite text outside the finite range can produce infinity.\nRounding follows the target float width.",
        ),
        _ => (
            "42",
            format!("42{name}"),
            "12x",
            "InvalidDigit",
            "Parses base-ten digits with an optional sign. Whitespace and radix prefixes\nare not trimmed or recognized. Invalid digits return `InvalidDigit`; a value\noutside the target range returns `OutOfRange`.",
        ),
    };
    let boundary = target.integer_layout().map_or(String::new(), |(bits, signed)| {
        let overflow = 1u128 << if signed { bits - 1 } else { bits };
        format!("\n    val overflow = match {name}::from_str(\"{overflow}\") {{\n        Err(ParseError::OutOfRange) => true,\n        _ => false,\n    }};")
    });
    let suffix = if boundary.is_empty() {
        ""
    } else {
        " && overflow"
    };
    example(
        &format!(
            "Parses a string as `{name}`.\n\n{detail}\nEmpty input returns `ParseError::Empty`. Parsing consumes the complete string\nand returns a checked `Result`; it does not change the input."
        ),
        &format!(
            "use core::num::ParseError;\nfn main() -> bool {{\n    val parsed = match {name}::from_str(\"{input}\") {{\n        Ok(value) => value == {output},\n        Err(_) => false,\n    }};\n    val empty = match {name}::from_str(\"\") {{\n        Err(ParseError::Empty) => true,\n        _ => false,\n    }};\n    val invalid = match {name}::from_str(\"{invalid}\") {{\n        Err(ParseError::{error}) => true,\n        _ => false,\n    }};{boundary}\n    parsed && empty && invalid{suffix}\n}}"
        ),
    )
}
