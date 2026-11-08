//! Exact special values and sign observations, without an invented precision bound.
pub(super) fn example(ty: &str, method: &str) -> String {
    let (bits, infinity, nan) = if ty == "f32" {
        ("u32", 0x7f80_0000u64, 0x7fc0_0001u64)
    } else {
        ("u64", 0x7ff0_0000_0000_0000u64, 0x7ff8_0000_0000_0001u64)
    };
    let nan = format!("{ty}::from_bits({nan}{bits})");
    let infinity = format!("{ty}::from_bits({infinity}{bits})");
    let expression = match method {
        "sin" => format!("0.0{ty}.sin() == 0.0{ty} && {infinity}.sin().is_nan()"),
        "cos" => format!("0.0{ty}.cos() == 1.0{ty} && {infinity}.cos().is_nan()"),
        "tan" => format!("0.0{ty}.tan() == 0.0{ty} && {infinity}.tan().is_nan()"),
        "asin" => format!("0.0{ty}.asin() == 0.0{ty} && 2.0{ty}.asin().is_nan()"),
        "acos" => format!("1.0{ty}.acos() == 0.0{ty} && 2.0{ty}.acos().is_nan()"),
        "atan" => format!("0.0{ty}.atan() == 0.0{ty} && {nan}.atan().is_nan()"),
        "atan2" => format!(
            "0.0{ty}.atan2(1.0{ty}) == 0.0{ty} &&\n        1.0{ty}.atan2(-1.0{ty}) > 0.0{ty} &&\n        (-1.0{ty}).atan2(-1.0{ty}) < 0.0{ty}"
        ),
        "exp" => format!("0.0{ty}.exp() == 1.0{ty} && (-{infinity}).exp() == 0.0{ty}"),
        "ln" => format!(
            "1.0{ty}.ln() == 0.0{ty} && (-1.0{ty}).ln().is_nan() && 0.0{ty}.ln().is_infinite()"
        ),
        "powf" => format!(
            "{nan}.powf(0.0{ty}) == 1.0{ty} &&\n        1.0{ty}.powf({nan}) == 1.0{ty} && (-1.0{ty}).powf(0.5{ty}).is_nan()"
        ),
        "powi" => format!(
            "(-1.0{ty}).powi(2147483647i32) == -1.0{ty} &&\n        (-1.0{ty}).powi(2147483646i32) == 1.0{ty} && {nan}.powi(0i32) == 1.0{ty}"
        ),
        _ => unreachable!("authored math operation"),
    };
    format!("fn main() -> bool {{\n    {expression}\n}}")
}
