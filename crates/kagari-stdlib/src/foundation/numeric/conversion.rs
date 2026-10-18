//! Numeric parsing and checked conversions.
use crate::foundation::enumerations::value::{self as enums, EnumSymbols};
use kagari_runtime::{
    error::RuntimeError,
    native::{binding::NativeResult, context::CallContext, scalar::NativeScalar},
    numeric,
    value::Value,
};
use kagari_types::{scalar::BuiltinType, ty::Ty};
use std::num::{IntErrorKind, ParseIntError};

fn invalid() -> RuntimeError {
    RuntimeError::module_validation("foundation construction contract")
}

fn result_item(cx: &CallContext<'_>) -> NativeResult<BuiltinType> {
    let Ty::Enum(nominal) = cx.result_type() else {
        return Err(invalid());
    };
    let args = &nominal.arguments;
    let Some(Ty::Builtin(target)) = args.first() else {
        return Err(invalid());
    };
    Ok(*target)
}

pub(crate) fn from_str(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
    let Value::Str(text) = cx.argument(0)? else {
        return Err(invalid());
    };
    let parsed = {
        let text = cx.heap().string(text).ok_or_else(invalid)?;
        parse(result_item(cx)?, &text)?
    };
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

fn result(
    symbols: &EnumSymbols,
    cx: &CallContext<'_>,
    value: Result<Value, &str>,
    error_type: &str,
) -> NativeResult<Value> {
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
                vec![],
            )?,
        ),
    };
    let _root = cx.heap().root_value(value).ok_or_else(invalid)?;
    enums::allocate(
        symbols,
        cx,
        &cx.result_type_argument()?,
        "Result",
        member,
        vec![value],
    )
}

pub(crate) fn try_from(symbols: &EnumSymbols, cx: &mut CallContext<'_>) -> NativeResult<Value> {
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

fn parse(target: BuiltinType, text: &str) -> NativeResult<Result<Value, u8>> {
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
    use kagari_runtime::value::Value;
    use kagari_types::scalar::BuiltinType;

    #[test]
    fn parsing_preserves_fixed_width_integer_boundaries() {
        for (kind, text, expected) in [
            (BuiltinType::U64, "4294967296", Value::U64(1u64 << 32)),
            (
                BuiltinType::U64,
                "18446744073709551615",
                Value::U64(u64::MAX),
            ),
            (
                BuiltinType::I64,
                "-9223372036854775808",
                Value::I64(i64::MIN),
            ),
            (
                BuiltinType::I64,
                "9223372036854775807",
                Value::I64(i64::MAX),
            ),
        ] {
            assert_eq!(parse(kind, text).unwrap(), Ok(expected));
        }
        for (kind, text) in [
            (BuiltinType::U64, "18446744073709551616"),
            (BuiltinType::I64, "-9223372036854775809"),
            (BuiltinType::I64, "9223372036854775808"),
        ] {
            assert_eq!(parse(kind, text).unwrap(), Err(2));
        }
    }
}
