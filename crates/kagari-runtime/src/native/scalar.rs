//! Rust scalar conversion is inferred only against an explicit Kagari signature.
//! This defines converters, never language declarations or trait implementations.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
    value::Value,
};
use kagari_abi::{scalar::BuiltinType, types::AbiType};

mod sealed {
    pub trait Scalar {}
}
pub trait NativeScalar: sealed::Scalar + Sized + 'static {
    fn abi_type() -> AbiType;
    fn decode(value: Value) -> NativeResult<Self>;
    fn encode(self) -> Value;
}
fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ModuleValidation,
        "native scalar conversion differs from its declaration",
    )
}

macro_rules! integer_scalar {
    ($rust:ty, $abi:ident, $value:ident, $physical:ty) => {
        impl sealed::Scalar for $rust {}
        impl NativeScalar for $rust {
            fn abi_type() -> AbiType {
                AbiType::Builtin(BuiltinType::$abi)
            }
            fn decode(value: Value) -> NativeResult<Self> {
                let Value::$value(value) = value else {
                    return Err(invalid());
                };
                Self::try_from(value).map_err(|_| invalid())
            }
            fn encode(self) -> Value {
                Value::$value(self as $physical)
            }
        }
    };
}
integer_scalar!(i8, I8, I32, i32);
integer_scalar!(i16, I16, I32, i32);
integer_scalar!(i32, I32, I32, i32);
integer_scalar!(i64, I64, I64, i64);
integer_scalar!(isize, ISize, I64, i64);
integer_scalar!(u8, U8, I64, i64);
integer_scalar!(u16, U16, I64, i64);
integer_scalar!(u32, U32, I64, i64);
integer_scalar!(u64, U64, U64, u64);
integer_scalar!(usize, USize, U64, u64);

macro_rules! scalar {
    ($rust:ty, $abi:ident, $value:ident) => {
        impl sealed::Scalar for $rust {}
        impl NativeScalar for $rust {
            fn abi_type() -> AbiType {
                AbiType::Builtin(BuiltinType::$abi)
            }
            fn decode(value: Value) -> NativeResult<Self> {
                let Value::$value(value) = value else {
                    return Err(invalid());
                };
                Ok(value)
            }
            fn encode(self) -> Value {
                Value::$value(self)
            }
        }
    };
}
scalar!(bool, Bool, Bool);
scalar!(f32, F32, F32);
scalar!(f64, F64, F64);
impl sealed::Scalar for () {}
impl NativeScalar for () {
    fn abi_type() -> AbiType {
        AbiType::Builtin(BuiltinType::Unit)
    }
    fn decode(value: Value) -> NativeResult<Self> {
        matches!(value, Value::Unit)
            .then_some(())
            .ok_or_else(invalid)
    }
    fn encode(self) -> Value {
        Value::Unit
    }
}
