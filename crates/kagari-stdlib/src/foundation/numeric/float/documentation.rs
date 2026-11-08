//! Width-specific observations of rounding, classification and bit transport.
pub(super) fn example(ty: &str, method: &str) -> String {
    let (bits, negative_zero, infinity, nan, one) = if ty == "f32" {
        (
            "u32",
            0x8000_0000u64,
            0x7f80_0000u64,
            0x7fc0_0001u64,
            0x3f80_0000u64,
        )
    } else {
        (
            "u64",
            0x8000_0000_0000_0000u64,
            0x7ff0_0000_0000_0000u64,
            0x7ff8_0000_0000_0001u64,
            0x3ff0_0000_0000_0000u64,
        )
    };
    let nan = format!("{ty}::from_bits({nan}{bits})");
    let inf = format!("{ty}::from_bits({infinity}{bits})");
    let expression = match method {
        "abs" => format!("(-3.5{ty}).abs() == 3.5{ty} && (-0.0{ty}).abs().to_bits() == 0{bits}"),
        "sqrt" => format!(
            "9.0{ty}.sqrt() == 3.0{ty} && (-1.0{ty}).sqrt().is_nan() && (-0.0{ty}).sqrt().to_bits() == {negative_zero}{bits}"
        ),
        "floor" => format!("1.75{ty}.floor() == 1.0{ty} && (-1.25{ty}).floor() == -2.0{ty}"),
        "ceil" => format!("1.25{ty}.ceil() == 2.0{ty} && (-1.75{ty}).ceil() == -1.0{ty}"),
        "round" => format!("1.5{ty}.round() == 2.0{ty} && (-1.5{ty}).round() == -2.0{ty}"),
        "trunc" => format!("1.75{ty}.trunc() == 1.0{ty} && (-1.75{ty}).trunc() == -1.0{ty}"),
        "fract" => format!("1.25{ty}.fract() == 0.25{ty} && (-1.25{ty}).fract() == -0.25{ty}"),
        "is_nan" => format!("{nan}.is_nan() && !{inf}.is_nan() && !0.0{ty}.is_nan()"),
        "is_infinite" => {
            format!("{inf}.is_infinite() && (-{inf}).is_infinite() && !{nan}.is_infinite()")
        }
        "is_finite" => format!("1.25{ty}.is_finite() && !{inf}.is_finite() && !{nan}.is_finite()"),
        "min" => format!("(-2.0{ty}).min(3.0{ty}) == -2.0{ty} && {nan}.min(2.0{ty}) == 2.0{ty}"),
        "max" => format!("(-2.0{ty}).max(3.0{ty}) == 3.0{ty} && 2.0{ty}.max({nan}) == 2.0{ty}"),
        "clamp" => format!(
            "10.0{ty}.clamp(0.0{ty}, 5.0{ty}) == 5.0{ty} &&\n        3.0{ty}.clamp(0.0{ty}, 5.0{ty}) == 3.0{ty} &&\n        {nan}.clamp(0.0{ty}, 5.0{ty}).is_nan()"
        ),
        "to_bits" => format!(
            "1.0{ty}.to_bits() == {one}{bits} && (-0.0{ty}).to_bits() == {negative_zero}{bits}"
        ),
        "from_bits" => format!(
            "{ty}::from_bits({negative_zero}{bits}).to_bits() == {negative_zero}{bits} &&\n        {nan}.is_nan() && {nan}.to_bits() == {}{bits}",
            if ty == "f32" {
                0x7fc0_0001u64
            } else {
                0x7ff8_0000_0000_0001u64
            }
        ),
        _ => unreachable!("authored float utility"),
    };
    let expression = expression.replace(" && ", " &&\n        ");
    format!("fn main() -> bool {{\n    {expression}\n}}")
}
