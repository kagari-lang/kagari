use crate::{
    RuntimeError,
    gc::GcHeap,
    value::{EnumTag, Value},
};
use kagari_ir::module::abi::BuiltinType;
use std::num::IntErrorKind;
use std::num::ParseIntError;

pub(crate) fn parse(
    gc: &GcHeap,
    ty: BuiltinType,
    args: &[Value],
    radix: bool,
) -> Result<Value, RuntimeError> {
    let Some(Value::Str(text)) = args.first() else {
        return Err(RuntimeError::module_validation("invalid parse receiver"));
    };
    let result = if radix {
        let [_, Value::I64(base)] = args else {
            return Err(RuntimeError::module_validation("invalid radix operand"));
        };
        if !(2..=36).contains(base) {
            Err(3)
        } else {
            integer(text, ty, *base as u32)
        }
    } else {
        if args.len() != 1 {
            return Err(RuntimeError::module_validation("invalid parser arity"));
        }
        match ty {
            BuiltinType::Bool => text
                .parse::<bool>()
                .map(Value::Bool)
                .map_err(|_| if text.is_empty() { 0 } else { 4 }),
            BuiltinType::F32 => text
                .parse::<f32>()
                .map(Value::F32)
                .map_err(|_| if text.is_empty() { 0 } else { 4 }),
            BuiltinType::F64 => text
                .parse::<f64>()
                .map(Value::F64)
                .map_err(|_| if text.is_empty() { 0 } else { 4 }),
            _ if ty.integer_layout().is_some() => integer(text, ty, 10),
            _ => return Err(RuntimeError::module_validation("unsupported parse type")),
        }
    };
    let (tag, payload) = match result {
        Ok(value) => (EnumTag::ResultOk, value),
        Err(error) => (
            EnumTag::ResultErr,
            Value::Enum(gc.alloc_enum(EnumTag::ParseError(error), vec![])?),
        ),
    };
    gc.alloc_enum(tag, vec![payload]).map(Value::Enum)
}

fn integer(text: &str, ty: BuiltinType, base: u32) -> Result<Value, u8> {
    let failure = |error: ParseIntError| match error.kind() {
        IntErrorKind::Empty => 0,
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => 2,
        _ => 1,
    };
    match ty {
        BuiltinType::I8 => i8::from_str_radix(text, base)
            .map(|v| Value::I32(v.into()))
            .map_err(failure),
        BuiltinType::I16 => i16::from_str_radix(text, base)
            .map(|v| Value::I32(v.into()))
            .map_err(failure),
        BuiltinType::I32 => i32::from_str_radix(text, base)
            .map(Value::I32)
            .map_err(failure),
        BuiltinType::I64 | BuiltinType::ISize => i64::from_str_radix(text, base)
            .map(Value::I64)
            .map_err(failure),
        BuiltinType::U8 => u8::from_str_radix(text, base)
            .map(|v| Value::I64(v.into()))
            .map_err(failure),
        BuiltinType::U16 => u16::from_str_radix(text, base)
            .map(|v| Value::I64(v.into()))
            .map_err(failure),
        BuiltinType::U32 => u32::from_str_radix(text, base)
            .map(|v| Value::I64(v.into()))
            .map_err(failure),
        BuiltinType::U64 | BuiltinType::USize => u64::from_str_radix(text, base)
            .map(Value::U64)
            .map_err(failure),
        _ => Err(4),
    }
}
