use super::execute_checks;
use rstest::rstest;

#[rstest]
#[case::i8("i8")]
#[case::i16("i16")]
#[case::i32("i32")]
#[case::i64("i64")]
#[case::u8("u8")]
#[case::u16("u16")]
#[case::u32("u32")]
#[case::u64("u64")]
fn all_integer_widths_expose_the_finite_policy_inventory(#[case] ty: &str) {
    let mut checks = Vec::new();
    let literal = |value| format!("{value}{ty}");
    // Small exact inputs prove concrete method/result ownership, not kernels.
    for (method, receiver, rhs, expected) in [
        ("wrapping_add", 5, 3, 8),
        ("wrapping_sub", 5, 3, 2),
        ("wrapping_mul", 5, 3, 15),
        ("saturating_add", 5, 3, 8),
        ("saturating_sub", 5, 3, 2),
        ("saturating_mul", 5, 3, 15),
    ] {
        checks.push(format!(
            "{{ val result: {ty} = ({}).{method}({}); result == {} }}",
            literal(receiver),
            literal(rhs),
            literal(expected),
        ));
    }
    for (method, expected) in [
        ("checked_add", 8),
        ("checked_sub", 2),
        ("checked_mul", 15),
        ("checked_div", 1),
        ("checked_rem", 2),
    ] {
        checks.push(format!(
            "{{ val result: Option<{ty}> = (5{ty}).{method}(3{ty}); \
             match result {{ Some(value) => value == {}, None => false }} }}",
            literal(expected),
        ));
    }
    for (method, expected) in [
        ("overflowing_add", 8),
        ("overflowing_sub", 2),
        ("overflowing_mul", 15),
    ] {
        checks.push(format!(
            "{{ val result: ({ty}, bool) = (5{ty}).{method}(3{ty}); \
             val (value, flag) = result; value == {} && !flag }}",
            literal(expected),
        ));
    }
    for (method, receiver, expected) in [("rotate_left", 5, 10), ("rotate_right", 4, 2)] {
        checks.push(format!(
            "{{ val result: {ty} = ({}).{method}(1u32); result == {} }}",
            literal(receiver),
            literal(expected),
        ));
    }
    if let Some(signed) = ty.strip_prefix('u') {
        checks.push(format!(
            "{{ val offset: {ty} = (0{ty}).wrapping_add_signed(-1i{signed}); \
             offset == !0{ty} }}"
        ));
    }
    execute_checks(checks);
}

#[rstest]
#[case::i8("i8", 8)]
#[case::i16("i16", 16)]
#[case::i32("i32", 32)]
#[case::i64("i64", 64)]
#[case::u8("u8", 8)]
#[case::u16("u16", 16)]
#[case::u32("u32", 32)]
#[case::u64("u64", 64)]
fn rotations_use_u32_counts_modulo_each_receiver_width(#[case] ty: &str, #[case] bits: u32) {
    let mut checks = Vec::new();
    for (method, expected) in [
        ("rotate_left", format!("2{ty}")),
        ("rotate_right", format!("(1{ty} << {}u32)", bits - 1)),
    ] {
        for count in [1, bits + 1, 2 * bits + 1] {
            checks.push(format!("(1{ty}).{method}({count}u32) == {expected}"));
        }
        for count in [0, bits, 2 * bits] {
            checks.push(format!("(1{ty}).{method}({count}u32) == 1{ty}"));
        }
    }
    // u32::MAX reduces to width - 1 for all supported power-of-two widths.
    checks.push(format!("(1{ty}).rotate_right(4294967295u32) == 2{ty}"));
    if ty.starts_with('i') {
        checks.push(format!("(-1{ty}).rotate_left({}u32) == -1{ty}", bits + 3));
    }
    execute_checks(checks);
}
