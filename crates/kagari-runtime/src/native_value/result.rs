//! Script Result values keep their rooted identity and original error trace.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{
    gc::RootSet,
    native_module::types::TypeExpression,
    native_value::representation::NativeRepresentation,
    value::{EnumTag, Value},
};
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use std::marker::PhantomData;

/// A script Result value, distinct from NativeResult's native execution failure.
/// Reading and returning this handle preserve Err provenance; creating a fresh
/// Err captures the current execution origin through the normal heap allocator.
pub struct NativeResultValue<T: ?Sized, E> {
    call: NativeCall,
    root: RootSet,
    ty: AbiType,
    _types: PhantomData<fn(&T, E)>,
}

impl<T: NativeValue, E: NativeValue> NativeResultValue<T, E> {
    pub fn is_ok(&self) -> NativeResult<bool> {
        let Value::Enum(id) = self.root.get(0).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        match self.call.heap.enum_snapshot(id).ok_or_else(invalid)?.tag {
            EnumTag::ResultOk => Ok(true),
            EnumTag::ResultErr => Ok(false),
            _ => Err(invalid()),
        }
    }
    /// Read only the selected success payload; an Err keeps its original object.
    pub fn unwrap_or(&self, fallback: T) -> NativeResult<T> {
        let [ok, _] = arguments(&self.ty)? else {
            return Err(invalid());
        };
        let Value::Enum(id) = self.root.get(0).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        let snapshot = self.call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.as_slice()) {
            (EnumTag::ResultOk, [value]) => T::read(&self.call, value.clone(), ok),
            (EnumTag::ResultErr, [_]) => Ok(fallback),
            _ => Err(invalid()),
        }
    }
    pub fn from_result(call: &NativeCall, value: Result<T, E>) -> NativeResult<Self> {
        let expected = call.result_type();
        let [ok, error] = arguments(expected)? else {
            return Err(invalid());
        };
        let (tag, payload) = match value {
            Ok(value) => (EnumTag::ResultOk, value.write(call, ok)?),
            Err(value) => (EnumTag::ResultErr, value.write(call, error)?),
        };
        Self::read(
            call,
            Value::Enum(call.heap.alloc_enum(tag, vec![payload])?),
            expected,
        )
    }

    /// Decode the selected branch without changing the Result object or its trace.
    pub fn payload(&self) -> NativeResult<Result<T, E>> {
        let [ok, error] = arguments(&self.ty)? else {
            return Err(invalid());
        };
        let Value::Enum(id) = self.root.get(0).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        let snapshot = self.call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        let [value] = snapshot.fields.as_slice() else {
            return Err(invalid());
        };
        match snapshot.tag {
            EnumTag::ResultOk => Ok(Ok(T::read(&self.call, value.clone(), ok)?)),
            EnumTag::ResultErr => Ok(Err(E::read(&self.call, value.clone(), error)?)),
            _ => Err(invalid()),
        }
    }
}

impl<T: NativeValue, E: NativeValue> NativeRepresentation for NativeResultValue<T, E> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Enum(StandardEnum::Result);
    const VARIANT_NAMES: &'static [&'static str] = &["Ok", "Err"];
}

impl<T: NativeValue, E: NativeValue> NativeValue for NativeResultValue<T, E> {
    fn type_expression(generics: &[&'static str]) -> TypeExpression {
        TypeExpression::Named {
            path: vec!["Result"],
            arguments: vec![T::type_expression(generics), E::type_expression(generics)],
            bindings: vec![],
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        arguments(expected)?;
        call.check(&value, expected)?;
        Ok(Self {
            call: call.clone(),
            root: call
                .heap
                .root_execution_values(vec![value])
                .ok_or_else(invalid)?,
            ty: expected.clone(),
            _types: PhantomData,
        })
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        call.compatible(&self.call)?;
        if *expected != self.ty {
            return Err(invalid());
        }
        let value = self.root.get(0).ok_or_else(invalid)?;
        call.check(&value, expected)?;
        call.retain(value)
    }
}

fn arguments(ty: &AbiType) -> NativeResult<&[AbiType]> {
    match ty {
        AbiType::StandardEnum {
            kind: StandardEnum::Result,
            args,
        } if args.len() == 2 => Ok(args),
        _ => Err(invalid()),
    }
}
