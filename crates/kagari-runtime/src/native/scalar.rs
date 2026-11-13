//! Rust scalar conversion is inferred only against an explicit Kagari signature.
//! This defines converters, never language declarations or trait implementations.
use crate::{
    error::{RuntimeError, reason::ErrorReason},
    native::binding::NativeResult,
    value::view::ValueView,
};

use kagari_types::scalar::BuiltinType;

mod sealed {
    pub trait Scalar {}
}

/// Converts a Rust scalar to or from its declared script type.
///
/// Rust `char` uses the distinct script `char` domain, never an integer carrier.
/// Rust `usize` and `isize` use script `u64` and `i64`, respectively. Decoding
/// checks the Rust host range before callback entry; hosts needing the complete
/// script domain must accept `u64`/`i64`. Encoding widens losslessly on supported
/// 32-bit and 64-bit hosts. These conveniences do not provide sequence views.
/// An unrepresentable machine scalar returns `HostCallFailure`; a mismatched
/// value representation remains a `ModuleValidation` error.
pub trait NativeScalar: sealed::Scalar + Sized + 'static {
    fn abi_type() -> BuiltinType;

    fn decode(value: ValueView<'_>) -> NativeResult<Self>;

    fn encode<'call>(self) -> ValueView<'call>;
}

fn invalid() -> RuntimeError {
    RuntimeError::new(ErrorReason::NativeScalarMismatch)
}

macro_rules! integer_scalar {
    ($rust:ty, $abi:ident, $accessor:ident, $physical:ty) => {
        integer_scalar!($rust, $abi, $accessor, $physical, invalid);
    };
    ($rust:ty, $abi:ident, $accessor:ident, $physical:ty, $range_error:expr) => {
        impl sealed::Scalar for $rust {}

        impl NativeScalar for $rust {
            fn abi_type() -> BuiltinType {
                BuiltinType::$abi
            }

            fn decode(value: ValueView<'_>) -> NativeResult<Self> {
                let Some(value) = value.$accessor() else {
                    return Err(invalid());
                };
                Self::try_from(value).map_err(|_| $range_error())
            }

            fn encode<'call>(self) -> ValueView<'call> {
                ValueView::from(self as $physical)
            }
        }
    };
}

integer_scalar!(i8, I8, as_i32, i32);
integer_scalar!(i16, I16, as_i32, i32);
integer_scalar!(i32, I32, as_i32, i32);
integer_scalar!(i64, I64, as_i64, i64);
integer_scalar!(isize, I64, as_i64, i64, || RuntimeError::host_call_failure(
    "i64 value is outside the Rust isize conversion range"
));
integer_scalar!(u8, U8, as_i64, i64);
integer_scalar!(u16, U16, as_i64, i64);
integer_scalar!(u32, U32, as_i64, i64);
integer_scalar!(u64, U64, as_u64, u64);
integer_scalar!(usize, U64, as_u64, u64, || RuntimeError::host_call_failure(
    "u64 value is outside the Rust usize conversion range"
));

macro_rules! scalar {
    ($rust:ty, $abi:ident, $accessor:ident) => {
        impl sealed::Scalar for $rust {}

        impl NativeScalar for $rust {
            fn abi_type() -> BuiltinType {
                BuiltinType::$abi
            }

            fn decode(value: ValueView<'_>) -> NativeResult<Self> {
                let Some(value) = value.$accessor() else {
                    return Err(invalid());
                };
                Ok(value)
            }

            fn encode<'call>(self) -> ValueView<'call> {
                ValueView::from(self)
            }
        }
    };
}

scalar!(bool, Bool, as_bool);
scalar!(char, Char, as_char);
scalar!(f32, F32, as_f32);
scalar!(f64, F64, as_f64);

impl sealed::Scalar for () {}

impl NativeScalar for () {
    fn abi_type() -> BuiltinType {
        BuiltinType::Unit
    }

    fn decode(value: ValueView<'_>) -> NativeResult<Self> {
        value.is_unit().then_some(()).ok_or_else(invalid)
    }

    fn encode<'call>(self) -> ValueView<'call> {
        ValueView::unit()
    }
}
