//! Tuple values retain one script value shape, independently of callback argument packs.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{native_module::types::TypeExpression, value::Value};
use kagari_abi::types::AbiType;

macro_rules! tuple {
    ($length:literal; $($ty:ident: $value:ident: $expected:ident),+) => {
        impl<$($ty: NativeValue),+> NativeValue for ($($ty,)+) {
            fn type_expression(generics: &[&'static str]) -> TypeExpression {
                TypeExpression::Tuple(vec![$($ty::type_expression(generics)),+])
            }
            fn read(call: &NativeCall, value: Value, expected: &AbiType) -> NativeResult<Self> {
                call.check(&value, expected)?;
                let AbiType::Tuple(types) = expected else { return Err(invalid()); };
                let [$($expected),+] = types.as_slice() else { return Err(invalid()); };
                // A later field conversion may collect; keep every input field live.
                let _root = call.heap.root_execution_values(vec![value.clone()]).ok_or_else(invalid)?;
                let Value::Tuple(values) = value else { return Err(invalid()); };
                let [$($value),+]: [Value; $length] = values.try_into().map_err(|_| invalid())?;
                Ok(($($ty::read(call, $value, $expected)?,)+))
            }
            fn write(self, call: &NativeCall, expected: &AbiType) -> NativeResult<Value> {
                let AbiType::Tuple(types) = expected else { return Err(invalid()); };
                let [$($expected),+] = types.as_slice() else { return Err(invalid()); };
                let ($($value,)+) = self;
                let value = Value::Tuple(vec![$($value.write(call, $expected)?),+]);
                call.check(&value, expected)?;
                call.retain(value)
            }
        }
    };
}
tuple!(1; A: a: a_type);
tuple!(2; A: a: a_type, B: b: b_type);
tuple!(3; A: a: a_type, B: b: b_type, C: c: c_type);
tuple!(4; A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type);
tuple!(5; A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type);
tuple!(6; A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type);
tuple!(7; A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type, G: g: g_type);
tuple!(8; A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type, G: g: g_type, H: h: h_type);
