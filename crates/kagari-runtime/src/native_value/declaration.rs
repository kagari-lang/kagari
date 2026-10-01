//! Symbolic trait slots used by generated signature metadata. Value conversion
//! delegates to the same rooted, checked proxy as open script generic values.
use crate::{
    native_module::types::TypeExpression,
    native_value::{GenericValue, NativeCall, NativeResult, NativeValue, named},
    value::Value,
};
use kagari_abi::types::AbiType;

#[doc(hidden)]
pub struct AssociatedValue<const SLOT: usize>(GenericValue<0>);
impl<const SLOT: usize> NativeValue for AssociatedValue<SLOT> {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        TypeExpression::Associated(SLOT)
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        GenericValue::read(call, value, expected).map(Self)
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        self.0.write(call, expected)
    }
}

#[doc(hidden)]
pub struct SelfValue(GenericValue<0>);
impl NativeValue for SelfValue {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        named("Self")
    }
    fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
        GenericValue::read(call, value, expected).map(Self)
    }
    fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
        self.0.write(call, expected)
    }
}
