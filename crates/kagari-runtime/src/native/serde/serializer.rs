//! Direct Serde value construction; child conversions use the normal rooted scope.
pub(super) mod compound;
mod text;
use super::unsupported;
use crate::{
    error::RuntimeError,
    frame::types::arguments::TypeArgument,
    native::{
        binding::NativeResult,
        catalog::DeclarationCatalog,
        conversion::{
            IntoKagari, KagariType, context::ConversionContext, structures::StructEncoder,
        },
        serde::serializer::compound::{Pack, Record},
        types::Type,
    },
    value::Value,
};
use serde::{
    Serialize,
    ser::{Impossible, Serializer as SerdeSerializer},
};
use std::fmt::Display;

pub(super) struct Serializer<'scope, 'runtime> {
    cx: &'scope mut ConversionContext<'runtime>,
    expected: &'scope TypeArgument,
}

struct SerializeValue<'a, T: ?Sized>(&'a T);

impl<T: ?Sized> KagariType for SerializeValue<'_, T> {
    fn kagari_type(_: &DeclarationCatalog) -> NativeResult<Type> {
        Err(unsupported("serialization proxy has no standalone schema"))
    }
}

impl<T: Serialize + ?Sized> IntoKagari for SerializeValue<'_, T> {
    fn into_kagari(
        self,
        cx: &mut ConversionContext<'_>,
        expected: &TypeArgument,
    ) -> NativeResult<Value> {
        self.0.serialize(Serializer::new(cx, expected))
    }
}

pub(super) fn encode<T: Serialize + ?Sized>(
    cx: &mut ConversionContext<'_>,
    expected: &TypeArgument,
    value: &T,
) -> NativeResult<Value> {
    // The enclosing registered schema supplies this child's type. The actual
    // serialization model and resulting value still check against it on access.
    cx.encode_prepared(expected, SerializeValue(value))
}

impl<'scope, 'runtime> Serializer<'scope, 'runtime> {
    pub(super) fn new(
        cx: &'scope mut ConversionContext<'runtime>,
        expected: &'scope TypeArgument,
    ) -> Self {
        Self { cx, expected }
    }

    fn scalar<T: IntoKagari>(self, value: T) -> NativeResult<Value> {
        self.cx.encode_value(self.expected, value)
    }
}

macro_rules! scalar {
    ($($method:ident: $ty:ty),+) => { $(
        fn $method(self, value: $ty) -> NativeResult<Value> { self.scalar(value) }
    )+ };
}

impl<'scope, 'runtime> SerdeSerializer for Serializer<'scope, 'runtime> {
    type Ok = Value;
    type Error = RuntimeError;
    type SerializeSeq = Pack<'scope, 'runtime>;
    type SerializeTuple = Pack<'scope, 'runtime>;
    type SerializeTupleStruct = Impossible<Value, RuntimeError>;
    type SerializeTupleVariant = Pack<'scope, 'runtime>;
    type SerializeMap = Impossible<Value, RuntimeError>;
    type SerializeStruct = Record<'scope, 'runtime>;
    type SerializeStructVariant = Impossible<Value, RuntimeError>;

    scalar!(serialize_bool: bool, serialize_i8: i8, serialize_i16: i16,
        serialize_i32: i32, serialize_i64: i64, serialize_u8: u8, serialize_u16: u16,
        serialize_u32: u32, serialize_u64: u64, serialize_f32: f32, serialize_f64: f64);

    fn serialize_i128(self, _: i128) -> NativeResult<Value> {
        Err(unsupported("i128"))
    }

    fn serialize_u128(self, _: u128) -> NativeResult<Value> {
        Err(unsupported("u128"))
    }

    fn serialize_char(self, value: char) -> NativeResult<Value> {
        self.scalar(value)
    }

    fn serialize_str(self, value: &str) -> NativeResult<Value> {
        self.cx.check_type::<String>(self.expected)?;
        self.cx.charge_string(value.len())?;
        let mut text = String::new();
        text.try_reserve_exact(value.len())
            .map_err(|_| RuntimeError::resource_limit("Serde string capacity"))?;
        text.push_str(value);
        self.cx.runtime().gc().alloc_string(text)
    }

    fn collect_str<T: Display + ?Sized>(self, value: &T) -> NativeResult<Value> {
        self.cx.check_type::<String>(self.expected)?;
        text::collect(self.cx, value)
    }

    fn serialize_bytes(self, _: &[u8]) -> NativeResult<Value> {
        Err(unsupported("byte encoding; use a sequence schema"))
    }

    fn serialize_none(self) -> NativeResult<Value> {
        compound::check_standard_enum(self.cx, self.expected, false)?;
        Pack::variant(self.cx, self.expected, "None", 0)?.finish()
    }

    fn serialize_some<T: Serialize + ?Sized>(self, value: &T) -> NativeResult<Value> {
        compound::check_standard_enum(self.cx, self.expected, false)?;
        let mut pack = Pack::variant(self.cx, self.expected, "Some", 1)?;
        pack.push(value)?;
        pack.finish()
    }

    fn serialize_unit(self) -> NativeResult<Value> {
        self.scalar(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> NativeResult<Value> {
        Err(unsupported("unit struct"))
    }

    fn serialize_unit_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
    ) -> NativeResult<Value> {
        Pack::variant(self.cx, self.expected, variant, 0)?.finish()
    }

    fn serialize_newtype_struct<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: &T,
    ) -> NativeResult<Value> {
        Err(unsupported("newtype struct"))
    }

    fn serialize_newtype_variant<T: Serialize + ?Sized>(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> NativeResult<Value> {
        let mut pack = Pack::variant(self.cx, self.expected, variant, 1)?;
        pack.push(value)?;
        pack.finish()
    }

    fn serialize_seq(self, len: Option<usize>) -> NativeResult<Self::SerializeSeq> {
        Pack::sequence(self.cx, self.expected, len)
    }

    fn serialize_tuple(self, len: usize) -> NativeResult<Self::SerializeTuple> {
        Pack::tuple(self.cx, self.expected, len)
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> NativeResult<Self::SerializeTupleStruct> {
        Err(unsupported("tuple struct"))
    }

    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        len: usize,
    ) -> NativeResult<Self::SerializeTupleVariant> {
        Pack::variant(self.cx, self.expected, variant, len)
    }

    fn serialize_map(self, _: Option<usize>) -> NativeResult<Self::SerializeMap> {
        Err(unsupported("map"))
    }

    fn serialize_struct(self, _: &'static str, len: usize) -> NativeResult<Self::SerializeStruct> {
        let encoder: StructEncoder<'scope, 'runtime> = self.cx.write_struct(self.expected)?;
        Ok(Record::new(encoder, len))
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> NativeResult<Self::SerializeStructVariant> {
        Err(unsupported("record variant"))
    }
}
