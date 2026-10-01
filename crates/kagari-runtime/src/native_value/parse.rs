//! Checked primitive parser errors with the declared closed enum representation.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{
    native_module::types::TypeExpression,
    native_value::representation::NativeRepresentation,
    value::{EnumTag, Value},
};
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use std::num::{IntErrorKind, ParseIntError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseFailure {
    Empty,
    InvalidDigit,
    OutOfRange,
    InvalidRadix,
    InvalidSyntax,
}

impl ParseFailure {
    pub(crate) fn integer(error: ParseIntError) -> Self {
        match error.kind() {
            IntErrorKind::Empty => Self::Empty,
            IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => Self::OutOfRange,
            _ => Self::InvalidDigit,
        }
    }
}

impl NativeRepresentation for ParseFailure {
    const CONSTRUCTOR: NativeTypeConstructor =
        NativeTypeConstructor::Enum(StandardEnum::ParseError);
    const VARIANT_NAMES: &'static [&'static str] = &[
        "Empty",
        "InvalidDigit",
        "OutOfRange",
        "InvalidRadix",
        "InvalidSyntax",
    ];
}

impl NativeValue for ParseFailure {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        TypeExpression::Named {
            path: vec!["ParseError"],
            arguments: vec![],
            bindings: vec![],
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        check_type(expected)?;
        call.check(&value, expected)?;
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        if !snapshot.fields.is_empty() {
            return Err(invalid());
        }
        match snapshot.tag {
            EnumTag::ParseError(0) => Ok(Self::Empty),
            EnumTag::ParseError(1) => Ok(Self::InvalidDigit),
            EnumTag::ParseError(2) => Ok(Self::OutOfRange),
            EnumTag::ParseError(3) => Ok(Self::InvalidRadix),
            EnumTag::ParseError(4) => Ok(Self::InvalidSyntax),
            _ => Err(invalid()),
        }
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        check_type(expected)?;
        let index = match self {
            Self::Empty => 0,
            Self::InvalidDigit => 1,
            Self::OutOfRange => 2,
            Self::InvalidRadix => 3,
            Self::InvalidSyntax => 4,
        };
        let value = Value::Enum(call.heap.alloc_enum(EnumTag::ParseError(index), vec![])?);
        call.check(&value, expected)?;
        call.retain(value)
    }
}

fn check_type(expected: &AbiType) -> NativeResult<()> {
    match expected {
        AbiType::StandardEnum {
            kind: StandardEnum::ParseError,
            args,
        } if args.is_empty() => Ok(()),
        _ => Err(invalid()),
    }
}
