//! Each enum case describes its own payload and meaning.
pub(in crate::foundation) fn variant(owner: &str, name: &str) -> &'static str {
    match (owner, name) {
        ("Option", "Some") => "Contains the present value of type `T`.",
        ("Option", "None") => {
            "Represents absence and carries no value. `?` returns this case early from an Option-returning function."
        }
        ("Result", "Ok") => "Contains the successful value of type `T`.",
        ("Result", "Err") => {
            "Contains the error of type `E`. `?` converts and returns this error early from the enclosing compatible function."
        }
        ("Ordering", "Less") => "The left operand is ordered before the right operand.",
        ("Ordering", "Equal") => "The operands compare equal in the selected total ordering.",
        ("Ordering", "Greater") => "The left operand is ordered after the right operand.",
        ("Bound", "Included") => "The endpoint value belongs to the range.",
        ("Bound", "Excluded") => "The endpoint value does not belong to the range.",
        ("Bound", "Unbounded") => {
            "There is no endpoint on this side of the range; no value is stored."
        }
        ("ControlFlow", "Continue") => "Contains the normal continuation value of type `T`.",
        ("ControlFlow", "Break") => "Contains the early-exit value of type `B`.",
        ("ParseError", "Empty") => "The input string contained no characters.",
        ("ParseError", "InvalidDigit") => {
            "An integer input contained a character not accepted as a decimal digit or sign at that position."
        }
        ("ParseError", "OutOfRange") => {
            "The integer text denotes a value outside the destination type's range."
        }
        ("ParseError", "InvalidRadix") => {
            "Reserved for an invalid radix request. The current decimal `FromStr` implementations do not emit this case."
        }
        ("ParseError", "InvalidSyntax") => {
            "The input does not match the floating-point or boolean parser's syntax."
        }
        ("TryFromIntError", "OutOfRange") => {
            "The source value is not representable by the requested checked conversion."
        }
        ("CharTryFromError", "InvalidScalar") => {
            "The integer is a surrogate in U+D800..U+DFFF or exceeds U+10FFFF, so it is not a Unicode scalar."
        }
        ("SpawnError", "ScopeClosed") => {
            "The host has closed the scope. No factory is queued and no Task is admitted."
        }
        ("SpawnError", "CapacityExceeded") => {
            "The scope or runtime has no remaining Task admission capacity. No factory is queued."
        }
        ("SpawnError", "DispatchUnavailable") => {
            "The host notification route cannot accept the admission. No Task is published to the caller."
        }
        _ => unreachable!("a public enum variant needs authored documentation"),
    }
}
