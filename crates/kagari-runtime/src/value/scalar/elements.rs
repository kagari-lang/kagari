//! Exact Rust primitives for compact fixed-array storage, without Host codecs.
use crate::value::{carriers::ScopedValue, scalar::domain};
use kagari_contract::representation::builtin_representation;
use kagari_gc::error::GcError;
use kagari_types::scalar::BuiltinType;

/// A physical primitive has one normalized script representation and a checked
/// semantic domain. Machine-sized Host integer conversion remains Native policy.
pub(crate) trait Primitive: Copy {
    const TYPE: BuiltinType;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError>;

    fn encode<'gc>(self) -> ScopedValue<'gc>;
}

fn payload(value: ScopedValue<'_>, ty: BuiltinType) -> Result<u64, GcError> {
    let (representation, bits) = value.scalar_payload().ok_or(GcError::Representation)?;
    if representation != builtin_representation(ty)
        || !domain::admits_payload(representation, Some(ty), bits)
    {
        return Err(GcError::Representation);
    }
    Ok(bits)
}

macro_rules! integers {
    ($($rust:ty: $builtin:ident => $carrier:ty),* $(,)?) => {$(
        impl Primitive for $rust {
            const TYPE: BuiltinType = BuiltinType::$builtin;

            fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
                // The exact signed/unsigned domain is checked before narrowing.
                Ok(payload(value, Self::TYPE)? as Self)
            }

            fn encode<'gc>(self) -> ScopedValue<'gc> {
                ScopedValue::from(self as $carrier)
            }
        }
    )*};
}

integers!(i8:I8 => i32, i16:I16 => i32, i32:I32 => i32, i64:I64 => i64,
    u8:U8 => i64, u16:U16 => i64, u32:U32 => i64, u64:U64 => u64);

impl Primitive for () {
    const TYPE: BuiltinType = BuiltinType::Unit;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
        payload(value, Self::TYPE).map(|_| ())
    }

    fn encode<'gc>(self) -> ScopedValue<'gc> {
        ScopedValue::unit()
    }
}

impl Primitive for bool {
    const TYPE: BuiltinType = BuiltinType::Bool;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
        Ok(payload(value, Self::TYPE)? != 0)
    }

    fn encode<'gc>(self) -> ScopedValue<'gc> {
        ScopedValue::from(self)
    }
}

impl Primitive for char {
    const TYPE: BuiltinType = BuiltinType::Char;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
        domain::decode_char(payload(value, Self::TYPE)?).ok_or(GcError::Representation)
    }

    fn encode<'gc>(self) -> ScopedValue<'gc> {
        ScopedValue::from(self)
    }
}

impl Primitive for f32 {
    const TYPE: BuiltinType = BuiltinType::F32;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
        Ok(Self::from_bits(payload(value, Self::TYPE)? as u32))
    }

    fn encode<'gc>(self) -> ScopedValue<'gc> {
        ScopedValue::from(self)
    }
}

impl Primitive for f64 {
    const TYPE: BuiltinType = BuiltinType::F64;

    fn decode(value: ScopedValue<'_>) -> Result<Self, GcError> {
        Ok(Self::from_bits(payload(value, Self::TYPE)?))
    }

    fn encode<'gc>(self) -> ScopedValue<'gc> {
        ScopedValue::from(self)
    }
}
