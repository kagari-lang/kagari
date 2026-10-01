//! Concrete Rust types supply both metadata and checked conversions, including aliases.
use super::{NativeCall, NativeResult, NativeValue, invalid, named};
use crate::{
    native_module::types::TypeExpression,
    value::{EnumTag, Value},
};
use kagari_abi::{scalar::BuiltinType, standard::surface::StandardEnum, types::AbiType};
use std::cmp::Ordering;

macro_rules! scalar {
    ($rust:ty, $name:literal, $abi:ident, $value:ident, $wire:ty) => {
        impl NativeValue for $rust {
            fn type_expression(_: &[&'static str]) -> TypeExpression {
                named($name)
            }
            fn read(_: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
                if *expected != AbiType::Builtin(BuiltinType::$abi) {
                    return Err(invalid());
                }
                if let Value::$value(value) = value {
                    Self::try_from(value).map_err(|_| invalid())
                } else {
                    Err(invalid())
                }
            }
            fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
                if *expected != AbiType::Builtin(BuiltinType::$abi) {
                    return Err(invalid());
                }
                call.retain(Value::$value(
                    <$wire>::try_from(self).map_err(|_| invalid())?,
                ))
            }
        }
    };
}
scalar!(bool, "bool", Bool, Bool, bool);
scalar!(i8, "i8", I8, I32, i32);
scalar!(i16, "i16", I16, I32, i32);
scalar!(i32, "i32", I32, I32, i32);
scalar!(i64, "i64", I64, I64, i64);
scalar!(isize, "isize", ISize, I64, i64);
scalar!(u8, "u8", U8, U64, u64);
scalar!(u16, "u16", U16, U64, u64);
scalar!(u32, "u32", U32, U64, u64);
scalar!(u64, "u64", U64, U64, u64);
scalar!(usize, "usize", USize, U64, u64);
scalar!(f32, "f32", F32, F32, f32);
scalar!(f64, "f64", F64, F64, f64);
scalar!(String, "String", String, Str, String);

impl NativeValue for Ordering {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        named("Ordering")
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        if *expected
            != (AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                args: vec![],
            })
        {
            return Err(invalid());
        }
        call.check(&value, expected)?;
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        if !snapshot.fields.is_empty() {
            return Err(invalid());
        }
        match snapshot.tag {
            EnumTag::OrderingLess => Ok(Self::Less),
            EnumTag::OrderingEqual => Ok(Self::Equal),
            EnumTag::OrderingGreater => Ok(Self::Greater),
            _ => Err(invalid()),
        }
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        if *expected
            != (AbiType::StandardEnum {
                kind: StandardEnum::Ordering,
                args: vec![],
            })
        {
            return Err(invalid());
        }
        let tag = match self {
            Self::Less => EnumTag::OrderingLess,
            Self::Equal => EnumTag::OrderingEqual,
            Self::Greater => EnumTag::OrderingGreater,
        };
        call.retain(Value::Enum(call.heap.alloc_enum(tag, vec![])?))
    }
}

impl NativeValue for () {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        TypeExpression::Tuple(vec![])
    }
    fn read(_: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        if value == Value::Unit && *expected == AbiType::Builtin(BuiltinType::Unit) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        if *expected == AbiType::Builtin(BuiltinType::Unit) {
            call.retain(Value::Unit)
        } else {
            Err(invalid())
        }
    }
}
impl<T: NativeValue> NativeValue for Option<T> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::Named {
            path: vec!["Option"],
            arguments: vec![T::type_expression(generics)],
            bindings: vec![],
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        let AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args,
        } = expected
        else {
            return Err(invalid());
        };
        call.check(&value, expected)?;
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.as_slice()) {
            (EnumTag::OptionNone, []) => Ok(None),
            (EnumTag::OptionSome, [value]) => Ok(Some(T::read(
                call,
                value.clone(),
                args.first()
                    .filter(|_| args.len() == 1)
                    .ok_or_else(invalid)?,
            )?)),
            _ => Err(invalid()),
        }
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        let AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args,
        } = expected
        else {
            return Err(invalid());
        };
        let (tag, fields) = match self {
            Some(value) => (
                EnumTag::OptionSome,
                vec![
                    value.write(
                        call,
                        args.first()
                            .filter(|_| args.len() == 1)
                            .ok_or_else(invalid)?,
                    )?,
                ],
            ),
            None => (EnumTag::OptionNone, vec![]),
        };
        call.retain(Value::Enum(call.heap.alloc_enum(tag, fields)?))
    }
}
