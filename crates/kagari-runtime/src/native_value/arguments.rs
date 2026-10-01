//! An outer Rust tuple describes callback arguments without allocating a script tuple.
use super::{NativeCall, NativeResult, NativeValue, invalid};
use crate::{native_module::types::TypeExpression, value::Value};
use kagari_abi::types::AbiType;

/// A callback's complete argument list: () for none, (T,) for one, (A, B) for two.
/// Each element is one checked script value. Conversion preserves left-to-right
/// order and retains earlier heap values while converting later arguments.
pub trait NativeArguments: 'static {
    fn type_expressions(generics: &[&'static str]) -> Vec<TypeExpression>;
    fn into_values(self, call: &NativeCall, expected: &[AbiType]) -> NativeResult<Vec<Value>>;
}

impl NativeArguments for () {
    fn type_expressions(_: &[&'static str]) -> Vec<TypeExpression> {
        vec![]
    }
    fn into_values(self, _: &NativeCall, expected: &[AbiType]) -> NativeResult<Vec<Value>> {
        if expected.is_empty() {
            Ok(vec![])
        } else {
            Err(invalid())
        }
    }
}

macro_rules! arguments {
    ($($ty:ident: $value:ident: $expected:ident),+) => {
        impl<$($ty: NativeValue),+> NativeArguments for ($($ty,)+) {
            fn type_expressions(generics: &[&'static str]) -> Vec<TypeExpression> {
                vec![$($ty::type_expression(generics)),+]
            }
            fn into_values(self, call: &NativeCall, expected: &[AbiType]) -> NativeResult<Vec<Value>> {
                let [$($expected),+] = expected else { return Err(invalid()); };
                let ($($value,)+) = self;
                Ok(vec![$($value.write(call, $expected)?),+])
            }
        }
    };
}
arguments!(A: a: a_type);
arguments!(A: a: a_type, B: b: b_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type, G: g: g_type);
arguments!(A: a: a_type, B: b: b_type, C: c: c_type, D: d: d_type, E: e: e_type, F: f: f_type, G: g: g_type, H: h: h_type);
