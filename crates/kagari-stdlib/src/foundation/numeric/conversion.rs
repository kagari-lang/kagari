//! Numeric parsing and checked conversions.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::NativeCall, scalar::NativeScalar},
    numeric,
    value::view::ValueView,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::num::{IntErrorKind, ParseIntError};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

fn result_item(cx: &NativeCall<'_>) -> NativeResult<BuiltinType> {
    let Ty::Enum(nominal) = cx.result_type() else {
        return Err(invalid());
    };
    let args = &nominal.arguments;
    let Some(Ty::Builtin(target)) = args.first() else {
        return Err(invalid());
    };
    Ok(*target)
}

pub(crate) fn from_str<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    let text = cx.argument(0)?;
    let kind = result_item(cx)?;
    let parsed = cx.read_strings(&[text], |[text]| parse(kind, text))??;
    result(
        symbols,
        cx,
        parsed.map_err(|error| {
            [
                "Empty",
                "InvalidDigit",
                "OutOfRange",
                "InvalidRadix",
                "InvalidSyntax",
            ][error as usize]
        }),
        "ParseError",
    )
}

fn result<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
    value: Result<ValueView<'call>, &str>,
    error_type: &str,
) -> NativeResult<ValueView<'call>> {
    let (member, value) = match value {
        Ok(value) => ("Ok", value),
        Err(member) => (
            "Err",
            enums::allocate(
                symbols,
                cx,
                &cx.result_type_parameter(1)?,
                error_type,
                member,
                &[],
            )?,
        ),
    };
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        "Result",
        member,
        &[value],
    )
}

pub(crate) fn try_from<'call>(
    symbols: &EnumSymbols,
    cx: &mut NativeCall<'call>,
) -> NativeResult<ValueView<'call>> {
    let Ty::Builtin(source) = cx.argument_type(0)? else {
        return Err(invalid());
    };
    let converted = numeric::checked_convert(*source, result_item(cx)?, cx.argument(0)?)?;
    result(
        symbols,
        cx,
        converted.ok_or("OutOfRange"),
        "TryFromIntError",
    )
}

fn integer_error(error: ParseIntError) -> u8 {
    match error.kind() {
        IntErrorKind::Empty => 0,
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => 2,
        _ => 1,
    }
}

fn parse<'call>(target: BuiltinType, text: &str) -> NativeResult<Result<ValueView<'call>, u8>> {
    macro_rules! integers {
        ($($kind:ident: $rust:ty),+) => {
            match target {
                $(BuiltinType::$kind => Ok(text.parse::<$rust>().map(NativeScalar::encode).map_err(integer_error)),)+
                BuiltinType::F32 => Ok(text.parse::<f32>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                BuiltinType::F64 => Ok(text.parse::<f64>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                BuiltinType::Bool => Ok(text.parse::<bool>().map(NativeScalar::encode).map_err(|_| if text.is_empty() {0} else {4})),
                _ => Err(invalid()),
            }
        };
    }

    integers!(I8:i8,I16:i16,I32:i32,I64:i64,U8:u8,U16:u16,U32:u32,U64:u64)
}

#[cfg(test)]
mod tests {
    use super::parse;
    use kagari_runtime::native::scalar::NativeScalar;
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn parsing_preserves_fixed_width_integer_boundaries() {
        for (kind, text, expected) in [
            (BuiltinType::U64, "4294967296", (1u64 << 32).encode()),
            (BuiltinType::U64, "18446744073709551615", u64::MAX.encode()),
            (BuiltinType::I64, "-9223372036854775808", i64::MIN.encode()),
            (BuiltinType::I64, "9223372036854775807", i64::MAX.encode()),
        ] {
            let actual = parse(kind, text).unwrap().unwrap();
            if kind == BuiltinType::U64 {
                assert_eq!(actual.as_u64(), expected.as_u64());
            } else {
                assert_eq!(actual.as_i64(), expected.as_i64());
            }
        }
        for (kind, text) in [
            (BuiltinType::U64, "18446744073709551616"),
            (BuiltinType::I64, "-9223372036854775809"),
            (BuiltinType::I64, "9223372036854775808"),
        ] {
            assert_eq!(parse(kind, text).unwrap().unwrap_err(), 2);
        }
    }
}
