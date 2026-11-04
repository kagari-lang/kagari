//! Rust scalar conversion is inferred only against an explicit Kagari signature.
//! This defines converters, never language declarations or trait implementations.
use crate::{
    error::{RuntimeError, RuntimeErrorKind},
    native::binding::NativeResult,
    value::{Value, scalar::ScalarValue},
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
/// Value carrier remains a `ModuleValidation` error.
pub trait NativeScalar: sealed::Scalar + Sized + 'static {
    fn abi_type() -> BuiltinType;

    fn decode(value: Value) -> NativeResult<Self>;

    fn encode(self) -> Value;
}

/// Engine-issued evidence that a Rust type uses the sealed scalar codec.
///
/// Private construction prevents custom conversion implementations from claiming
/// effect-free transport. The codec retains checked narrowing on input and cannot
/// manufacture managed references. Ordinary Native authors do not construct it.
pub struct ScalarConversion<T> {
    encode: fn(T) -> Value,
    decode: fn(Value) -> NativeResult<T>,
}

impl<T: NativeScalar> ScalarConversion<T> {
    pub(crate) fn new() -> Self {
        Self {
            encode: T::encode,
            decode: T::decode,
        }
    }
}

impl<T> ScalarConversion<T> {
    pub(crate) fn encode(self, value: T) -> Value {
        (self.encode)(value)
    }

    pub(crate) fn decode(self, value: Value) -> NativeResult<T> {
        (self.decode)(value)
    }
}

/// One scalar codec; production Value and scoped transport only adapt carriers.
pub(crate) trait ScalarCodec: Sized {
    fn decode_scalar(value: ScalarValue) -> Result<Self, ScalarError>;

    fn encode_scalar(self) -> ScalarValue;
}

/// Physical conversion failures carry no execution trace, String or host policy.
pub(crate) enum ScalarError {
    Representation,
    SignedHostRange,
    UnsignedHostRange,
}

impl ScalarError {
    fn into_error(self) -> RuntimeError {
        match self {
            Self::Representation => invalid(),
            Self::SignedHostRange => RuntimeError::host_call_failure(
                "i64 value is outside the Rust isize conversion range",
            ),
            Self::UnsignedHostRange => RuntimeError::host_call_failure(
                "u64 value is outside the Rust usize conversion range",
            ),
        }
    }
}

fn invalid() -> RuntimeError {
    RuntimeError::new(
        RuntimeErrorKind::ModuleValidation,
        "native scalar conversion differs from its declaration",
    )
}

macro_rules! integer_scalar {
    ($rust:ty, $abi:ident, $value:ident, $physical:ty) => {
        integer_scalar!($rust, $abi, $value, $physical, ScalarError::Representation);
    };
    ($rust:ty, $abi:ident, $value:ident, $physical:ty, $range_error:expr) => {
        native_scalar!($rust, $abi);

        impl ScalarCodec for $rust {
            #[inline]
            fn decode_scalar(value: ScalarValue) -> Result<Self, ScalarError> {
                let ScalarValue::$value(value) = value else {
                    return Err(ScalarError::Representation);
                };
                Self::try_from(value).map_err(|_| $range_error)
            }

            #[inline]
            fn encode_scalar(self) -> ScalarValue {
                ScalarValue::$value(self as $physical)
            }
        }
    };
}

macro_rules! native_scalar {
    ($rust:ty, $abi:ident) => {
        impl sealed::Scalar for $rust {}

        impl NativeScalar for $rust {
            fn abi_type() -> BuiltinType {
                BuiltinType::$abi
            }

            fn decode(value: Value) -> NativeResult<Self> {
                Self::decode_scalar(ScalarValue::from_value(value).ok_or_else(invalid)?)
                    .map_err(ScalarError::into_error)
            }

            fn encode(self) -> Value {
                self.encode_scalar().into_value()
            }
        }
    };
}

integer_scalar!(i8, I8, I32, i32);
integer_scalar!(i16, I16, I32, i32);
integer_scalar!(i32, I32, I32, i32);
integer_scalar!(i64, I64, I64, i64);
integer_scalar!(isize, I64, I64, i64, ScalarError::SignedHostRange);
integer_scalar!(u8, U8, I64, i64);
integer_scalar!(u16, U16, I64, i64);
integer_scalar!(u32, U32, I64, i64);
integer_scalar!(u64, U64, U64, u64);
integer_scalar!(usize, U64, U64, u64, ScalarError::UnsignedHostRange);

macro_rules! scalar {
    ($rust:ty, $abi:ident, $value:ident) => {
        native_scalar!($rust, $abi);

        impl ScalarCodec for $rust {
            #[inline]
            fn decode_scalar(value: ScalarValue) -> Result<Self, ScalarError> {
                let ScalarValue::$value(value) = value else {
                    return Err(ScalarError::Representation);
                };
                Ok(value)
            }

            #[inline]
            fn encode_scalar(self) -> ScalarValue {
                ScalarValue::$value(self)
            }
        }
    };
}

scalar!(bool, Bool, Bool);
scalar!(char, Char, Char);
scalar!(f32, F32, F32);
scalar!(f64, F64, F64);

native_scalar!((), Unit);

impl ScalarCodec for () {
    #[inline]
    fn decode_scalar(value: ScalarValue) -> Result<Self, ScalarError> {
        matches!(value, ScalarValue::Unit)
            .then_some(())
            .ok_or(ScalarError::Representation)
    }

    #[inline]
    fn encode_scalar(self) -> ScalarValue {
        ScalarValue::Unit
    }
}
