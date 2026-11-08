//! Boundary witnesses for each fixed-width integer policy.
use kagari_types::{integer::IntegerMethod, scalar::BuiltinType};

pub(super) fn example(operation: IntegerMethod, receiver: BuiltinType, name: &str) -> String {
    let (width, signed) = match receiver {
        BuiltinType::I8 => (8, true),
        BuiltinType::I16 => (16, true),
        BuiltinType::I32 => (32, true),
        BuiltinType::I64 => (64, true),
        BuiltinType::U8 => (8, false),
        BuiltinType::U16 => (16, false),
        BuiltinType::U32 => (32, false),
        BuiltinType::U64 => (64, false),
        _ => unreachable!("integer policy receiver"),
    };
    let max = (1i128 << (width - i32::from(signed))) - 1;
    let min = if signed { -(1i128 << (width - 1)) } else { 0 };
    let ty = format!("{receiver:?}").to_ascii_lowercase();
    let max = format!("{max}{ty}");
    let min = format!("{min}{ty}");
    let wrapped_product = if signed {
        format!("-2{ty}")
    } else {
        format!("{}{}", (1u128 << width) - 2, ty)
    };
    let expression = match operation {
        IntegerMethod::WrappingAdd => format!("{max}.{name}(1{ty}) == {min}"),
        IntegerMethod::WrappingSub => format!("({min}).{name}(1{ty}) == {max}"),
        IntegerMethod::WrappingMul => format!("{max}.{name}(2{ty}) == {wrapped_product}"),
        IntegerMethod::CheckedAdd => {
            format!("2{ty}.{name}(3{ty}) == Some(5{ty}) && {max}.{name}(1{ty}) == None")
        }
        IntegerMethod::CheckedSub => {
            format!("7{ty}.{name}(2{ty}) == Some(5{ty}) && ({min}).{name}(1{ty}) == None")
        }
        IntegerMethod::CheckedMul => {
            format!("2{ty}.{name}(3{ty}) == Some(6{ty}) && {max}.{name}(2{ty}) == None")
        }
        IntegerMethod::CheckedDiv | IntegerMethod::CheckedRem => {
            let result = if matches!(operation, IntegerMethod::CheckedDiv) {
                3
            } else {
                1
            };
            let mut text =
                format!("7{ty}.{name}(2{ty}) == Some({result}{ty}) && 7{ty}.{name}(0{ty}) == None");
            if signed {
                text.push_str(&format!(" && ({min}).{name}(-1{ty}) == None"));
            }
            text
        }
        IntegerMethod::OverflowingAdd => {
            format!("{max}.{name}(1{ty}) == ({min}, true) && 2{ty}.{name}(3{ty}) == (5{ty}, false)")
        }
        IntegerMethod::OverflowingSub => format!("({min}).{name}(1{ty}) == ({max}, true)"),
        IntegerMethod::OverflowingMul => {
            format!("{max}.{name}(2{ty}) == ({wrapped_product}, true)")
        }
        IntegerMethod::SaturatingAdd => format!("{max}.{name}(1{ty}) == {max}"),
        IntegerMethod::SaturatingSub => format!("({min}).{name}(1{ty}) == {min}"),
        IntegerMethod::SaturatingMul => format!("{max}.{name}(2{ty}) == {max}"),
        IntegerMethod::WrappingAddSigned => format!("0{ty}.{name}(-1i{width}) == {max}"),
        IntegerMethod::RotateLeft => format!(
            "1{ty}.{name}({}u32) == 2{ty} && 1{ty}.{name}({width}u32) == 1{ty}",
            width + 1
        ),
        IntegerMethod::RotateRight => {
            let highest = if signed {
                min
            } else {
                format!("{}{ty}", 1u128 << (width - 1))
            };
            format!("1{ty}.{name}(1u32) == {highest} && 1{ty}.{name}({width}u32) == 1{ty}")
        }
    };
    let expression = expression.replace(" && ", " &&\n        ");
    format!("fn main() -> bool {{\n    {expression}\n}}")
}
