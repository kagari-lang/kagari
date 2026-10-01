//! Rooted optional values support borrowed methods without cloning Rust payloads.
use crate::{
    gc::RootSet,
    native_module::types::TypeExpression,
    native_value::{
        NativeCall, NativeResult, NativeValue, invalid, representation::NativeRepresentation,
    },
    value::{EnumTag, Value},
};
use kagari_abi::{
    standard::surface::StandardEnum,
    types::{AbiType, native::NativeTypeConstructor},
};
use std::marker::PhantomData;

pub struct NativeOptionValue<T> {
    call: NativeCall,
    root: RootSet,
    ty: AbiType,
    _item: PhantomData<T>,
}
impl<T: NativeValue> NativeOptionValue<T> {
    pub fn is_some(&self) -> NativeResult<bool> {
        let Value::Enum(id) = self.root.get(0).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        match self.call.heap.enum_snapshot(id).ok_or_else(invalid)?.tag {
            EnumTag::OptionSome => Ok(true),
            EnumTag::OptionNone => Ok(false),
            _ => Err(invalid()),
        }
    }
    pub fn unwrap_or(&self, fallback: T) -> NativeResult<T> {
        let item = argument(&self.ty)?;
        let Value::Enum(id) = self.root.get(0).ok_or_else(invalid)? else {
            return Err(invalid());
        };
        let snapshot = self.call.heap.enum_snapshot(id).ok_or_else(invalid)?;
        match (snapshot.tag, snapshot.fields.as_slice()) {
            (EnumTag::OptionSome, [value]) => T::read(&self.call, value.clone(), item),
            (EnumTag::OptionNone, []) => Ok(fallback),
            _ => Err(invalid()),
        }
    }
}
impl<T: NativeValue> NativeRepresentation for NativeOptionValue<T> {
    const CONSTRUCTOR: NativeTypeConstructor = NativeTypeConstructor::Enum(StandardEnum::Option);
    const VARIANT_NAMES: &'static [&'static str] = &["Some", "None"];
}
impl<T: NativeValue> NativeValue for NativeOptionValue<T> {
    fn type_expression(names: &[&'static str]) -> TypeExpression {
        TypeExpression::Named {
            path: vec!["Option"],
            arguments: vec![T::type_expression(names)],
            bindings: vec![],
        }
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        argument(expected)?;
        call.check(&value, expected)?;
        Ok(Self {
            call: call.clone(),
            root: call
                .heap
                .root_execution_values(vec![value])
                .ok_or_else(invalid)?,
            ty: expected.clone(),
            _item: PhantomData,
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
fn argument(ty: &AbiType) -> NativeResult<&AbiType> {
    match ty {
        AbiType::StandardEnum {
            kind: StandardEnum::Option,
            args,
        } if args.len() == 1 => Ok(&args[0]),
        _ => Err(invalid()),
    }
}
