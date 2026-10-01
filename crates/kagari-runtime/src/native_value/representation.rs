//! Closed engine representations exposed by actual checked native value adapters.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{
    native_module::types::TypeExpression,
    value::{EnumTag, Value},
};
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use std::{cmp::Ordering, ops::Bound};

/// A registered alias takes its representation from its resolved Rust value type.
pub trait NativeRepresentation: NativeValue {
    const CONSTRUCTOR: NativeTypeConstructor;
    const VARIANT_NAMES: &'static [&'static str] = &[];
}

impl NativeRepresentation for Ordering {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Enum(StandardEnum::Ordering);
    const VARIANT_NAMES: &'static [&'static str] = &["Less", "Equal", "Greater"];
}

impl NativeRepresentation for String {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::String;
}

impl<T: NativeValue> NativeRepresentation for Option<T> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Enum(StandardEnum::Option);
    const VARIANT_NAMES: &'static [&'static str] = &["Some", "None"];
}

impl<T: NativeValue> NativeRepresentation for Bound<T> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Enum(StandardEnum::Bound);
    const VARIANT_NAMES: &'static [&'static str] = &["Included", "Excluded", "Unbounded"];
}
impl<T: NativeValue> NativeValue for Bound<T> {
    fn type_expression(names: &[&'static str]) -> TypeExpression {
        TypeExpression::Named {
            path: vec!["Bound"],
            arguments: vec![T::type_expression(names)],
            bindings: vec![],
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        let item = bound_item(expected)?;
        call.check(&value, expected)?;
        let Value::Enum(id) = value else {
            return Err(invalid());
        };
        let snapshot = call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.as_slice()) {
            (EnumTag::BoundUnbounded, []) => Ok(Bound::Unbounded),
            (EnumTag::BoundIncluded, [value]) => {
                Ok(Bound::Included(T::read(call, value.clone(), item)?))
            }
            (EnumTag::BoundExcluded, [value]) => {
                Ok(Bound::Excluded(T::read(call, value.clone(), item)?))
            }
            _ => Err(invalid()),
        }
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        let item = bound_item(expected)?;
        let (tag, fields) = match self {
            Bound::Included(value) => (EnumTag::BoundIncluded, vec![value.write(call, item)?]),
            Bound::Excluded(value) => (EnumTag::BoundExcluded, vec![value.write(call, item)?]),
            Bound::Unbounded => (EnumTag::BoundUnbounded, vec![]),
        };
        let value = Value::Enum(call.heap.alloc_enum(tag, fields)?);
        call.check(&value, expected)?;
        call.retain(value)
    }
}
fn bound_item(ty: &AbiType) -> NativeResult<&AbiType> {
    match ty {
        AbiType::StandardEnum {
            kind: StandardEnum::Bound,
            args,
        } if args.len() == 1 => Ok(&args[0]),
        _ => Err(invalid()),
    }
}
