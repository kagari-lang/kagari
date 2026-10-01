//! The actual uninhabited Rust result of a native that cannot return a script value.
use crate::{
    native_module::types::TypeExpression,
    native_value::{NativeCall, NativeResult, NativeValue, invalid},
    value::Value,
};
use kagari_abi::types::AbiType;

/// Use NativeResult<NativeNever> for a fallible native that always traps.
/// No value can be constructed, read or written; registered signatures return !.
#[derive(Debug)]
pub enum NativeNever {}

impl NativeValue for NativeNever {
    fn type_expression(_: &[&'static str]) -> TypeExpression {
        TypeExpression::Never
    }
    fn read(_: &NativeCall, _: Value, _: &AbiType) -> NativeResult<Self> {
        Err(invalid())
    }
    fn write(self, _: &NativeCall, _: &AbiType) -> NativeResult<Value> {
        match self {}
    }
}
